import { StreamLanguage } from '@codemirror/language'

/** Minimal Zig highlighting for CodeMirror (no official lang package yet). */
export const zigLanguage = StreamLanguage.define({
  name: 'zig',
  startState: () => ({ tok: false }),
  token(stream) {
    if (stream.eatSpace()) return null

    if (stream.match('//')) {
      stream.skipToEnd()
      return 'comment'
    }

    if (stream.match('\\\\')) {
      stream.skipToEnd()
      return 'string'
    }

    if (stream.match('"') || stream.match("'")) {
      const q = stream.current()
      while (!stream.eol()) {
        if (stream.peek() === '\\') {
          stream.next()
          stream.next()
          continue
        }
        if (stream.next() === q) break
      }
      return 'string'
    }

    if (stream.match(/0x[0-9a-fA-F_]+|0b[01_]+|0o[0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?/)) {
      return 'number'
    }

    if (stream.match(/@[A-Za-z_][\w]*/)) return 'meta'
    if (stream.match(/[A-Za-z_][\w]*/)) {
      const w = stream.current()
      if (ZIG_KEYWORDS.has(w)) return 'keyword'
      if (ZIG_TYPES.has(w)) return 'typeName'
      if (w === 'true' || w === 'false' || w === 'null' || w === 'undefined') return 'atom'
      return 'variableName'
    }

    stream.next()
    return null
  },
  languageData: {
    commentTokens: { line: '//' },
  },
})

const ZIG_KEYWORDS = new Set([
  'addrspace', 'align', 'allowzero', 'and', 'anyframe', 'anytype', 'asm', 'async', 'await',
  'break', 'callconv', 'catch', 'comptime', 'const', 'continue', 'defer', 'else', 'enum',
  'errdefer', 'error', 'export', 'extern', 'fn', 'for', 'if', 'inline', 'linksection',
  'noalias', 'noinline', 'nosuspend', 'opaque', 'or', 'orelse', 'packed', 'pub', 'resume',
  'return', 'struct', 'suspend', 'switch', 'test', 'threadlocal', 'try', 'union', 'unreachable',
  'usingnamespace', 'var', 'volatile', 'while',
])

const ZIG_TYPES = new Set([
  'i8', 'i16', 'i32', 'i64', 'i128', 'isize',
  'u8', 'u16', 'u32', 'u64', 'u128', 'usize',
  'f16', 'f32', 'f64', 'f80', 'f128',
  'bool', 'void', 'noreturn', 'type', 'anyerror', 'comptime_int', 'comptime_float',
  'c_char', 'c_short', 'c_ushort', 'c_int', 'c_uint', 'c_long', 'c_ulong', 'c_longlong', 'c_ulonglong', 'c_longdouble',
])
