import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import i18n from './i18n'

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) return sources(path)
    if (!/\.tsx?$/.test(entry) || /\.test\.tsx?$/.test(entry)) return []
    return [path]
  })
}

/** Every key the console asks for, as written in the sources. */
function usedKeys(): string[] {
  const keys = new Set<string>()
  for (const path of sources(join(process.cwd(), 'src'))) {
    const source = readFileSync(path, 'utf8')
    for (const match of source.matchAll(/\bt\(\s*'([A-Za-z0-9_]+)'/g)) keys.add(match[1])
  }
  return [...keys].sort()
}

// The console is bilingual by contract: a key that exists in one language only
// renders as its raw name for half the users. Concurrent workstreams edit the
// locale file, so this is checked rather than assumed.
describe('translations', () => {
  it('resolves every key the console uses, in both languages', async () => {
    await i18n.changeLanguage('en')
    const missing: string[] = []
    for (const key of usedKeys()) {
      for (const language of ['en', 'zh-CN']) {
        if (!i18n.exists(key, { lng: language })) missing.push(`${language}:${key}`)
      }
    }
    expect(missing).toEqual([])
  })

  it('uses a key at all, so the guard cannot pass vacuously', () => {
    expect(usedKeys().length).toBeGreaterThan(200)
  })
})
