// Fails when code uses a translation key that is missing from locales/en.json (FR-054).
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const en = JSON.parse(readFileSync(new URL('../../locales/en.json', import.meta.url)))
const has = (key) =>
  key.split('.').reduce((o, k) => (o && k in o ? o[k] : undefined), en) !== undefined

const files = []
const walk = (dir) => {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f)
    if (statSync(p).isDirectory()) walk(p)
    else if (/\.(ts|tsx)$/.test(f) && !/\.test\./.test(f)) files.push(p)
  }
}
walk(new URL('../src', import.meta.url).pathname)

const missing = []
for (const file of files) {
  const src = readFileSync(file, 'utf8')
  for (const m of src.matchAll(/\bt\(\s*['"]([\w.]+)['"]/g)) {
    if (!has(m[1])) missing.push(`${file}: ${m[1]}`)
  }
}
// Hard-coded interface text: JSX text with letters, and literal labels people read or hear.
// Allowed: code-like examples the user types (listed here with the reason).
const ALLOWED = new Set([
  'placeholder="leave."', // audit-log action prefix example, typed as code
])
const hardcoded = []
for (const file of files.filter((f) => f.endsWith('.tsx') && !f.includes('/src/test/'))) {
  const lines = readFileSync(file, 'utf8').split('\n')
  lines.forEach((line, i) => {
    const code = line.replace(/\/\/.*$/, '').replace(/\{\/\*.*?\*\/\}/g, '')
    // `>Some words<` with no expression inside, outside TypeScript generics like Promise<void>.
    for (const m of code.matchAll(/>([^<>{}]*[A-Za-z]{2,}[^<>{}]*)</g)) {
      // TypeScript, not JSX: `Promise<void>`, `(e: React.ChangeEvent<…>`, `A | Promise<B>`.
      if (
        /^\s*\(|\||\w+:\s*\w/.test(m[1]) ||
        (/^\s*[\w.]+\s*$/.test(m[1]) && /=>|Promise|keyof|React\./.test(code))
      )
        continue
      hardcoded.push(`${file}:${i + 1}: text "${m[1].trim()}"`)
    }
    for (const m of code.matchAll(
      /\b(aria-label|placeholder|title|alt)="([^"]*[A-Za-z]{2,}[^"]*)"/g,
    )) {
      if (ALLOWED.has(`${m[1]}="${m[2]}"`)) continue
      hardcoded.push(`${file}:${i + 1}: ${m[1]}="${m[2]}"`)
    }
  })
}

if (missing.length || hardcoded.length) {
  if (missing.length) console.error('Missing translation keys:\n' + missing.join('\n'))
  if (hardcoded.length)
    console.error('Text must come from locales/en.json (FR-054):\n' + hardcoded.join('\n'))
  process.exit(1)
}
console.log(`i18n OK (${files.length} files checked)`)
