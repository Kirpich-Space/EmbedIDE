//! Reproducible microbenchmark: cargo bench -p embedide-editor --bench buffer
use embedide_editor::TextBuffer;
use std::{hint::black_box, time::Instant};

fn main() {
    println!("bytes,edits,construct_us,edit_p50_ns,edit_p95_ns,materialize_us");
    for size in [1024 * 1024, 10 * 1024 * 1024] {
        let source = "a".repeat(size);
        let start = Instant::now();
        let mut buffer = TextBuffer::new(source);
        let construct_us = start.elapsed().as_micros();
        let mut seed = 0x5eed_u64;
        let mut samples = Vec::new();
        for _ in 0..2000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let position = (seed >> 32) as usize % buffer.byte_len();
            let start = Instant::now();
            buffer.replace(position..position, black_box("x")).unwrap();
            samples.push(start.elapsed().as_nanos());
        }
        samples.sort_unstable();
        let start = Instant::now();
        let text = black_box(buffer.contents());
        let materialize_us = start.elapsed().as_micros();
        assert_eq!(text.len(), size + 2000);
        assert_eq!(text.bytes().filter(|b| *b == b'x').count(), 2000);
        println!(
            "{size},2000,{construct_us},{},{},{materialize_us}",
            samples[999], samples[1899]
        );
    }
}
