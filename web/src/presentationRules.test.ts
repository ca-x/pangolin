import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The presentation rules AGENTS.md pins for operational tables and motion:
 * normal table text is at least 14px, and every animation is composited
 * (`transform`/`opacity`), under 300ms, and switched off for reduced motion.
 * The design system keeps one deliberate exception — a skeleton sweeps a muted
 * gradient at 1.6s linear — so it is checked as the only animation allowed past
 * the budget, and it must sweep through a transform on its own layer rather than
 * repainting the block with `background-position`.
 */
const css = (name: string) => readFileSync(join(process.cwd(), 'src', name), 'utf8')

type Rule = { selector: string; body: string }
/** Comments are stripped first: one sitting above a selector is not part of it. */
const stripComments = (source: string) => source.replace(/\/\*[\s\S]*?\*\//g, '')
function rules(source: string): Rule[] {
  const found: Rule[] = []
  for (const match of stripComments(source).matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    found.push({ selector: match[1].trim(), body: match[2] })
  }
  return found
}
/** The body of every `@keyframes <name>` block, brace-matched so inner blocks do not truncate it. */
function keyframes(source: string): Array<{ name: string; body: string }> {
  const found: Array<{ name: string; body: string }> = []
  for (const match of source.matchAll(/@keyframes\s+([\w-]+)\s*\{/g)) {
    const start = match.index! + match[0].length
    let depth = 1
    let index = start
    while (index < source.length && depth > 0) {
      if (source[index] === '{') depth += 1
      if (source[index] === '}') depth -= 1
      index += 1
    }
    found.push({ name: match[1], body: source.slice(start, index - 1) })
  }
  return found
}
/** The properties a keyframe animates, with the `from`/`50%` step selectors stripped. */
function animatedProperties(body: string): string[] {
  const declarations = body.replace(/(?:from|to|\d+(?:\.\d+)?%)\s*\{/g, '{')
  return [...declarations.matchAll(/([a-zA-Z-]+)\s*:/g)].map((match) => match[1])
}
const durations = (value: string) => [...value.matchAll(/(\d+(?:\.\d+)?)(ms|s)\b/g)].map((match) => Number(match[1]) * (match[2] === 's' ? 1000 : 1))

describe('operational table text', () => {
  it('keeps every cell and mono cell at the 14px minimum', () => {
    const offenders: string[] = []
    for (const rule of rules(css('styles.css'))) {
      // Cell content only: the uppercase column label is a header, not table text.
      if (!/(^|[\s,])td\b|\.mono-cell/.test(rule.selector)) continue
      for (const match of rule.body.matchAll(/font-size:\s*([\d.]+)px/g)) {
        if (Number(match[1]) < 14) offenders.push(`${rule.selector} → ${match[1]}px`)
      }
    }
    expect(offenders).toEqual([])
  })
})

describe('the loading shimmer', () => {
  it('animates only composited properties, in every stylesheet', () => {
    const offenders: string[] = []
    for (const name of ['styles.css', 'materials.css']) {
      for (const frame of keyframes(css(name))) {
        for (const property of animatedProperties(frame.body)) {
          if (property !== 'transform' && property !== 'opacity') offenders.push(`${name} @keyframes ${frame.name} → ${property}`)
        }
      }
    }
    expect(offenders).toEqual([])
  })

  it('sweeps the skeleton through a transform on its own layer', () => {
    const source = css('styles.css')
    const skeleton = rules(source).find((rule) => rule.selector === '.skeleton')
    expect(skeleton?.body).toMatch(/position:\s*relative/)
    expect(skeleton?.body).toMatch(/overflow:\s*hidden/)
    const sweep = rules(source).find((rule) => rule.selector === '.skeleton::after')
    expect(sweep?.body).toMatch(/animation:\s*skeleton-sweep\s+1\.6s\s+linear\s+infinite/)
    expect(keyframes(source).find((frame) => frame.name === 'skeleton-sweep')?.body).toMatch(/transform:\s*translateX/)
  })

  it('stops the shimmer for reduced motion and keeps every other animation in budget', () => {
    const source = css('styles.css')
    const reduced = source.slice(source.indexOf('@media (prefers-reduced-motion: reduce)'))
    expect(reduced).toMatch(/\.skeleton::after\s*\{\s*animation:\s*none/)
    const overBudget = rules(source)
      .filter((rule) => /animation:/.test(rule.body) && !/skeleton-sweep/.test(rule.body))
      .flatMap((rule) => durations(rule.body).filter((value) => value > 300).map((value) => `${rule.selector} → ${value}ms`))
    expect(overBudget).toEqual([])
  })
})
