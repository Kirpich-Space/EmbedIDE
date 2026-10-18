import { completeFromList, snippetCompletion, type Completion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete'

const C_KEYWORDS = [
  'auto', 'break', 'case', 'char', 'const', 'continue', 'default', 'do', 'double',
  'else', 'enum', 'extern', 'float', 'for', 'goto', 'if', 'inline', 'int', 'long',
  'register', 'restrict', 'return', 'short', 'signed', 'sizeof', 'static', 'struct',
  'switch', 'typedef', 'union', 'unsigned', 'void', 'volatile', 'while',
  'uint8_t', 'uint16_t', 'uint32_t', 'uint64_t',
  'int8_t', 'int16_t', 'int32_t', 'int64_t', 'size_t', 'uintptr_t', 'bool', 'true', 'false',
  'NULL',
]

const CPP_EXTRA = [
  'class', 'namespace', 'template', 'typename', 'using', 'public', 'private', 'protected',
  'virtual', 'override', 'final', 'constexpr', 'noexcept', 'nullptr', 'new', 'delete',
  'try', 'catch', 'throw', 'friend', 'operator', 'this', 'mutable', 'explicit',
  'static_cast', 'reinterpret_cast', 'const_cast', 'dynamic_cast', 'std',
]

const RUST_KEYWORDS = [
  'as', 'async', 'await', 'break', 'const', 'continue', 'crate', 'dyn',
  'else', 'enum', 'extern', 'false', 'fn', 'for', 'if', 'impl', 'in', 'let', 'loop',
  'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return', 'self', 'Self', 'static',
  'struct', 'super', 'trait', 'true', 'type', 'unsafe', 'use', 'where', 'while',
  'u8', 'u16', 'u32', 'u64', 'usize', 'i8', 'i16', 'i32', 'i64', 'isize', 'f32', 'f64',
  'bool', 'char', 'str', 'Option', 'Result', 'Some', 'None', 'Ok', 'Err', 'Vec',
  'unwrap', 'expect', 'clone', 'Default', 'Debug',
  'cortex_m', 'cortex_m_rt', 'entry', 'nop', 'no_std', 'no_main',
]

const ASM_KEYWORDS = [
  '.syntax', '.cpu', '.thumb', '.thumb_func', '.global', '.section', '.word', '.byte',
  '.equ', '.include', 'ldr', 'str', 'mov', 'add', 'sub', 'mul', 'and', 'orr', 'eor',
  'lsl', 'lsr', 'asr', 'cmp', 'b', 'bl', 'bx', 'beq', 'bne', 'blt', 'bgt', 'push', 'pop',
  'nop', 'wfi', 'cpsid', 'cpsie', 'r0', 'r1', 'r2', 'r3', 'r4', 'r5', 'r6', 'r7',
  'r8', 'r9', 'r10', 'r11', 'r12', 'sp', 'lr', 'pc',
]

const ZIG_KEYWORDS = [
  'const', 'var', 'fn', 'pub', 'export', 'extern', 'try', 'catch', 'defer', 'errdefer',
  'if', 'else', 'while', 'for', 'switch', 'return', 'break', 'continue', 'unreachable',
  'struct', 'enum', 'union', 'opaque', 'error', 'asm', 'comptime', 'inline', 'callconv',
  'and', 'or', 'orelse', 'async', 'await', 'suspend', 'resume', 'test', 'usingnamespace',
]

function kw(words: string[]): Completion[] {
  return words.map(label => ({ label, type: 'keyword' }))
}

function langOptions(lang: string): Completion[] {
  if (lang === 'rust') {
    return [
      ...kw(RUST_KEYWORDS),
      snippetCompletion('fn ${name}(${args}) {\n\t${}\n}', { label: 'fn', type: 'keyword', detail: 'function' }),
      snippetCompletion('impl ${Type} {\n\t${}\n}', { label: 'impl', type: 'keyword', detail: 'impl' }),
      snippetCompletion('match ${expr} {\n\t${pat} => ${},\n}', { label: 'match', type: 'keyword', detail: 'match' }),
      snippetCompletion('struct ${Name} {\n\t${field}: ${Type},\n}', { label: 'struct', type: 'keyword', detail: 'struct' }),
      snippetCompletion('#[entry]\nfn main() -> ! {\n\t${}\n\tloop {}\n}', { label: 'entry', type: 'function', detail: 'cortex-m entry' }),
      snippetCompletion('let mut ${name} = ${value};', { label: 'let mut', type: 'keyword', detail: 'mutable binding' }),
    ]
  }
  if (lang === 'zig') {
    return [
      ...kw(ZIG_KEYWORDS),
      snippetCompletion('export fn _start() callconv(.C) noreturn {\n\t${}\n\twhile (true) {}\n}', { label: '_start', type: 'function', detail: 'bare-metal entry' }),
      snippetCompletion('pub fn ${name}(${args}) ${ret} {\n\t${}\n}', { label: 'pub fn', type: 'keyword', detail: 'function' }),
      snippetCompletion('const ${name} = ${value};', { label: 'const', type: 'keyword', detail: 'const' }),
      snippetCompletion('while (${cond}) {\n\t${}\n}', { label: 'while', type: 'keyword', detail: 'while' }),
    ]
  }
  if (lang === 'cpp') {
    return [
      ...kw([...C_KEYWORDS, ...CPP_EXTRA]),
      snippetCompletion('int main(void) {\n\t${}\n\treturn 0;\n}', { label: 'main', type: 'function', detail: 'int main' }),
      snippetCompletion('for (int ${i} = 0; ${i} < ${n}; ${i}++) {\n\t${}\n}', { label: 'for', type: 'keyword', detail: 'for loop' }),
      snippetCompletion('while (${cond}) {\n\t${}\n}', { label: 'while', type: 'keyword', detail: 'while' }),
      snippetCompletion('if (${cond}) {\n\t${}\n}', { label: 'if', type: 'keyword', detail: 'if' }),
      snippetCompletion('#include <${header}>', { label: 'include', type: 'keyword', detail: '#include' }),
      snippetCompletion('class ${Name} {\npublic:\n\t${}\n};', { label: 'class', type: 'keyword', detail: 'class' }),
    ]
  }
  if (lang === 'asm' || lang === 's' || lang === 'S') {
    return kw(ASM_KEYWORDS)
  }
  // c / headers / default
  return [
    ...kw(C_KEYWORDS),
    snippetCompletion('int main(void) {\n\t${}\n\treturn 0;\n}', { label: 'main', type: 'function', detail: 'int main' }),
    snippetCompletion('for (int ${i} = 0; ${i} < ${n}; ${i}++) {\n\t${}\n}', { label: 'for', type: 'keyword', detail: 'for loop' }),
    snippetCompletion('while (${cond}) {\n\t${}\n}', { label: 'while', type: 'keyword', detail: 'while' }),
    snippetCompletion('if (${cond}) {\n\t${}\n}', { label: 'if', type: 'keyword', detail: 'if' }),
    snippetCompletion('#include <${header}>', { label: 'include', type: 'keyword', detail: '#include' }),
    snippetCompletion('#ifdef ${NAME}\n${}\n#endif', { label: 'ifdef', type: 'keyword', detail: '#ifdef' }),
  ]
}

/** Collect identifiers already present in the buffer */
function bufferWordCompletions(context: CompletionContext): CompletionResult | null {
  const word = context.matchBefore(/[A-Za-z_][\w]*/)
  if (!word || (word.from === word.to && !context.explicit)) return null
  const seen = new Set<string>()
  const options: Completion[] = []
  const text = context.state.doc.toString()
  const re = /[A-Za-z_][\w]{2,}/g
  let m: RegExpExecArray | null
  while ((m = re.exec(text))) {
    const w = m[0]
    if (seen.has(w) || w === word.text) continue
    seen.add(w)
    options.push({ label: w, type: 'variable', boost: -1 })
  }
  if (!options.length) return null
  return { from: word.from, options, validFor: /^[\w]*$/ }
}

export function createCompletionSource(getLang: () => string) {
  return (context: CompletionContext): CompletionResult | null | Promise<CompletionResult | null> => {
    const word = context.matchBefore(/[.#]?[A-Za-z_][\w]*/)
    if (!word || (word.from === word.to && !context.explicit)) return null

    const langSource = completeFromList(langOptions(getLang()))
    const langResult = langSource(context)
    const buf = bufferWordCompletions(context)

    if (langResult && 'then' in langResult) {
      return Promise.resolve(langResult).then(r => mergeResults(r, buf, word.from))
    }
    return mergeResults(langResult, buf, word.from)
  }
}

function mergeResults(
  a: CompletionResult | null,
  b: CompletionResult | null,
  from: number,
): CompletionResult | null {
  if (!a && !b) return null
  const options = [...(a?.options || []), ...(b?.options || [])]
  // de-dupe by label, prefer higher boost / earlier
  const map = new Map<string, Completion>()
  for (const opt of options) {
    const prev = map.get(opt.label)
    if (!prev || (opt.boost || 0) > (prev.boost || 0)) map.set(opt.label, opt)
  }
  return {
    from: a?.from ?? b?.from ?? from,
    options: [...map.values()],
    validFor: /^[\w.]*$/,
  }
}
