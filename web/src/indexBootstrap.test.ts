import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * The first paint is a promise about the rest of the session. With the colour
 * mode resolved only by the bundle, a hard reload in dark mode flashed a cream
 * splash, and `<html lang>` stayed the hard-coded `zh-CN` until React ran — so
 * an English reader's screen reader announced a Chinese document. `index.html`
 * therefore resolves both from storage (and, for `system`, the OS preference)
 * before any module loads.
 */
const html = readFileSync(join(process.cwd(), 'index.html'), 'utf8')
const inline = [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)].map((match) => match[1])
const bootstrap = inline.find((source) => source.includes('pangolin-color-mode'))

/** Runs the inline bootstrap the way a browser would: before the bundle. */
const runBootstrap = () => {
  expect(bootstrap, 'index.html must carry an inline mode/language bootstrap').toBeTruthy()
  new Function(bootstrap as string)()
}

describe('the first paint is resolved before the bundle', () => {
  beforeEach(() => {
    localStorage.clear()
    delete document.documentElement.dataset.mode
    document.documentElement.lang = 'zh-CN'
  })

  it('paints the stored dark mode instead of a light splash', () => {
    localStorage.setItem('pangolin-color-mode', 'dark')
    runBootstrap()
    expect(document.documentElement.dataset.mode).toBe('dark')
  })

  it('resolves a stored system mode against the OS preference', () => {
    localStorage.setItem('pangolin-color-mode', 'system')
    vi.stubGlobal('matchMedia', () => ({ matches: true, addEventListener() {}, removeEventListener() {} }))
    runBootstrap()
    expect(document.documentElement.dataset.mode).toBe('dark')
  })

  it('defaults to light when nothing is stored and the OS prefers light', () => {
    runBootstrap()
    expect(document.documentElement.dataset.mode).toBe('light')
  })

  it('applies the stored language instead of the hard-coded zh-CN', () => {
    localStorage.setItem('pangolin-language', 'en')
    runBootstrap()
    expect(document.documentElement.lang).toBe('en')
  })

  it('follows the browser language when nothing is stored', () => {
    runBootstrap()
    expect(document.documentElement.lang).toBe(navigator.language.startsWith('zh') ? 'zh-CN' : 'en')
  })

  it('runs before the module bundle, which is the only way it can be first paint', () => {
    const at = html.indexOf(bootstrap as string)
    expect(at).toBeGreaterThan(-1)
    expect(at).toBeLessThan(html.indexOf('type="module"'))
  })

  it('survives a browser that refuses storage instead of blanking the console', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => { throw new Error('storage is blocked') },
      setItem: () => {},
    })
    expect(() => runBootstrap()).not.toThrow()
    expect(document.documentElement.dataset.mode).toBe('light')
  })
})
