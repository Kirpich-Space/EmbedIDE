//! Line indexing and extended grapheme motions; no renderer or display-width policy.
use crate::{vim::Motion, EditError, TextBuffer};
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct LineIndex {
    starts: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct LineChange {
    range: Range<usize>,
    inserted_len: usize,
    removed_breaks: Vec<usize>,
    inserted_breaks: Vec<usize>,
}

impl LineChange {
    pub fn append(&mut self, next: &Self) -> bool {
        if self.range.is_empty()
            && next.range.is_empty()
            && next.range.start == self.range.start + self.inserted_len
        {
            let previous_len = self.inserted_len;
            self.inserted_breaks.extend(
                next.inserted_breaks
                    .iter()
                    .map(|offset| previous_len + offset),
            );
            self.inserted_len += next.inserted_len;
            true
        } else {
            false
        }
    }
}

impl LineIndex {
    pub fn new(buffer: &TextBuffer) -> Self {
        let mut starts = vec![0];
        let mut base = 0;
        for chunk in buffer.chunks() {
            starts.extend(
                chunk
                    .bytes()
                    .enumerate()
                    .filter_map(|(i, b)| (b == b'\n').then_some(base + i + 1)),
            );
            base += chunk.len();
        }
        Self { starts }
    }

    pub fn change(&self, range: Range<usize>, text: &str) -> LineChange {
        let left = self.starts.partition_point(|start| *start <= range.start);
        let right = self.starts.partition_point(|start| *start <= range.end);
        let removed_breaks = self.starts[left..right]
            .iter()
            .map(|offset| offset - range.start)
            .collect();
        let inserted_breaks = text
            .bytes()
            .enumerate()
            .filter_map(|(i, b)| (b == b'\n').then_some(i + 1))
            .collect();
        LineChange {
            range,
            inserted_len: text.len(),
            removed_breaks,
            inserted_breaks,
        }
    }

    pub fn apply(&mut self, change: &LineChange, forward: bool) {
        let (removed_len, inserted_len, breaks) = if forward {
            (
                change.range.end - change.range.start,
                change.inserted_len,
                &change.inserted_breaks,
            )
        } else {
            (
                change.inserted_len,
                change.range.end - change.range.start,
                &change.removed_breaks,
            )
        };
        let start = change.range.start;
        let left = self.starts.partition_point(|offset| *offset <= start);
        let right = self
            .starts
            .partition_point(|offset| *offset <= start + removed_len);
        let tail = left + breaks.len();
        drop(
            self.starts
                .splice(left..right, breaks.iter().map(|offset| start + offset)),
        );
        for start in &mut self.starts[tail..] {
            *start = *start - removed_len + inserted_len;
        }
    }

    pub fn count(&self) -> usize {
        self.starts.len()
    }
    pub fn row(&self, offset: usize) -> usize {
        self.starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1)
    }
    pub fn start(&self, row: usize) -> usize {
        self.starts[row.min(self.count() - 1)]
    }
    pub fn end(&self, row: usize, buffer: &TextBuffer) -> usize {
        if row + 1 >= self.count() {
            return buffer.byte_len();
        }
        let mut end = self.starts[row + 1] - 1;
        if end > self.start(row) && buffer.byte_at(end - 1) == Some(b'\r') {
            end -= 1;
        }
        end
    }
    pub fn lines(&self, first: usize, last: usize, buffer: &TextBuffer) -> Range<usize> {
        let end = if last + 1 < self.count() {
            self.start(last + 1)
        } else {
            buffer.byte_len()
        };
        self.start(first)..end
    }
    pub fn column(&self, offset: usize, buffer: &TextBuffer) -> Result<usize, EditError> {
        let end = offset.min(self.end(self.row(offset), buffer));
        Ok(buffer
            .chunks_in(self.start(self.row(offset))..end)?
            .map(|s| s.chars().count())
            .sum())
    }
    pub fn at_column(
        &self,
        row: usize,
        column: usize,
        buffer: &TextBuffer,
    ) -> Result<usize, EditError> {
        let start = self.start(row);
        let mut offset = start;
        for _ in 0..column {
            if offset >= self.end(row, buffer) {
                break;
            }
            offset = buffer.next_grapheme(offset)?;
        }
        Ok(offset)
    }
    pub fn first_nonblank(&self, row: usize, buffer: &TextBuffer) -> Result<usize, EditError> {
        let mut offset = self.start(row);
        for ch in buffer
            .chunks_in(offset..self.end(row, buffer))?
            .flat_map(str::chars)
        {
            if !ch.is_whitespace() {
                break;
            }
            offset += ch.len_utf8();
        }
        Ok(offset)
    }
    pub fn clamp(
        &self,
        offset: usize,
        insert: bool,
        buffer: &TextBuffer,
    ) -> Result<usize, EditError> {
        let _ = buffer.chunks_in(offset..offset)?;
        let row = self.row(offset);
        let end = self.end(row, buffer);
        if !insert && end > self.start(row) && offset >= end {
            buffer.previous_grapheme(end)
        } else {
            buffer.grapheme_start(offset.min(end))
        }
    }
}

pub(crate) fn next_unit(buffer: &TextBuffer, offset: usize) -> Result<usize, EditError> {
    if buffer.byte_at(offset) == Some(b'\r') && buffer.byte_at(offset + 1) == Some(b'\n') {
        Ok(offset + 2)
    } else {
        buffer.next_grapheme(offset)
    }
}

pub(crate) fn previous_unit(buffer: &TextBuffer, offset: usize) -> Result<usize, EditError> {
    if offset >= 2
        && buffer.byte_at(offset - 2) == Some(b'\r')
        && buffer.byte_at(offset - 1) == Some(b'\n')
    {
        Ok(offset - 2)
    } else {
        buffer.previous_grapheme(offset)
    }
}

fn class(ch: char) -> u8 {
    if ch.is_whitespace() {
        0
    } else if ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
    }
}

fn kind(buffer: &TextBuffer, offset: usize) -> Result<Option<u8>, EditError> {
    Ok(buffer.char_at(offset)?.map(class))
}

fn word(buffer: &TextBuffer, mut pos: usize, motion: Motion) -> Result<usize, EditError> {
    match motion {
        Motion::WordBackward => {
            if pos == 0 {
                return Ok(0);
            }
            pos = buffer.previous_grapheme(pos)?;
            while pos > 0 && kind(buffer, pos)? == Some(0) {
                pos = buffer.previous_grapheme(pos)?;
            }
            let category = kind(buffer, pos)?;
            while pos > 0 {
                let previous = buffer.previous_grapheme(pos)?;
                if kind(buffer, previous)? != category {
                    break;
                }
                pos = previous;
            }
        }
        Motion::WordForward => {
            let category = kind(buffer, pos)?;
            if category != Some(0) {
                while pos < buffer.byte_len() && kind(buffer, pos)? == category {
                    pos = buffer.next_grapheme(pos)?;
                }
            } else {
                pos = next_unit(buffer, pos)?;
            }
            while pos < buffer.byte_len() && kind(buffer, pos)? == Some(0) {
                // An empty logical line is a word stop. Consume a starting
                // empty line above, but do not skip subsequent empty lines.
                let line_start = pos == 0 || buffer.byte_at(pos - 1) == Some(b'\n');
                let newline = buffer.byte_at(pos) == Some(b'\n')
                    || (buffer.byte_at(pos) == Some(b'\r')
                        && buffer.byte_at(pos + 1) == Some(b'\n'));
                if line_start && newline {
                    break;
                }
                pos = next_unit(buffer, pos)?;
            }
        }
        Motion::WordEnd => {
            let next = buffer.next_grapheme(pos)?;
            if kind(buffer, pos)? == Some(0) || kind(buffer, pos)? != kind(buffer, next)? {
                pos = next;
                while pos < buffer.byte_len() && kind(buffer, pos)? == Some(0) {
                    pos = buffer.next_grapheme(pos)?;
                }
            }
            let category = kind(buffer, pos)?;
            loop {
                let next = buffer.next_grapheme(pos)?;
                if next == pos || kind(buffer, next)? != category {
                    break;
                }
                pos = next;
            }
        }
        _ => {}
    }
    Ok(pos)
}

// `dw`/`yw` stop at EOL if their final word step would enter another line.
// Earlier steps still cross lines, and a step starting on an empty line may
// consume that line. Keep this endpoint separate from cursor clamping.
pub(crate) fn operator_word_forward(
    buffer: &TextBuffer,
    lines: &LineIndex,
    mut position: usize,
    count: u32,
) -> Result<usize, EditError> {
    for index in 0..count {
        let next = word(buffer, position, Motion::WordForward)?;
        if index + 1 == count && lines.row(next) > lines.row(position) {
            let end = lines.end(lines.row(position), buffer);
            if position < end {
                return Ok(end);
            }
        }
        if next == position {
            break;
        }
        position = next;
    }
    Ok(position)
}

pub(crate) fn resolve(
    buffer: &TextBuffer,
    lines: &LineIndex,
    cursor: usize,
    preferred: Option<usize>,
    motion: Motion,
    count: u32,
    insert: bool,
) -> Result<(usize, Option<usize>), EditError> {
    let row = lines.row(cursor);
    let end = lines.end(row, buffer);
    let mut position = cursor;
    let mut desired = None;
    match motion {
        Motion::Down | Motion::Up => {
            let column = match preferred {
                Some(column) => column,
                None => {
                    let mut offset = lines.start(row);
                    let mut column = 0;
                    while offset < cursor {
                        offset = buffer.next_grapheme(offset)?;
                        column += 1;
                    }
                    column
                }
            };
            let target = if motion == Motion::Down {
                row.saturating_add(count as usize).min(lines.count() - 1)
            } else {
                row.saturating_sub(count as usize)
            };
            position = lines.at_column(target, column, buffer)?;
            desired = Some(column);
        }
        Motion::Left | Motion::Right => {
            for _ in 0..count {
                let next = if motion == Motion::Left {
                    previous_unit(buffer, position)?.max(lines.start(row))
                } else {
                    next_unit(buffer, position)?.min(end)
                };
                if position == next {
                    break;
                }
                position = next;
            }
        }
        Motion::LineStart => position = lines.start(row),
        Motion::FirstNonBlank => position = lines.first_nonblank(row, buffer)?,
        Motion::LineEnd => {
            let target = row
                .saturating_add(count.saturating_sub(1) as usize)
                .min(lines.count() - 1);
            position = lines.end(target, buffer);
        }
        Motion::Line(number) => {
            position = lines.first_nonblank(
                (number as usize).saturating_sub(1).min(lines.count() - 1),
                buffer,
            )?
        }
        Motion::LastLine => position = lines.first_nonblank(lines.count() - 1, buffer)?,
        Motion::WordForward | Motion::WordBackward | Motion::WordEnd => {
            for _ in 0..count {
                let next = word(buffer, position, motion)?;
                if next == position {
                    break;
                }
                position = next;
            }
        }
    }
    Ok((lines.clamp(position, insert, buffer)?, desired))
}

pub(crate) fn inner_word(
    buffer: &TextBuffer,
    cursor: usize,
    count: u32,
) -> Result<Range<usize>, EditError> {
    let category = kind(buffer, cursor)?;
    let mut start = cursor;
    while start > 0 {
        let previous = buffer.previous_grapheme(start)?;
        if kind(buffer, previous)? != category {
            break;
        }
        start = previous;
    }
    let mut end = cursor;
    for index in 0..count {
        if end >= buffer.byte_len() {
            break;
        }
        if index > 0 {
            while end < buffer.byte_len() && kind(buffer, end)? == Some(0) {
                end = buffer.next_grapheme(end)?;
            }
        }
        let category = kind(buffer, end)?;
        while end < buffer.byte_len() && kind(buffer, end)? == category {
            end = buffer.next_grapheme(end)?;
        }
    }
    Ok(start..end)
}
