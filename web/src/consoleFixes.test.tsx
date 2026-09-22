import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { MantineProvider } from '@mantine/core'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { EnabledPill, InlineQueryError, Status } from './components'
import { buildResolver, buildTheme } from './mantine'
import { resolvePalette, type PaletteTokens } from './palettes'

/**
 * Defects a jsdom render cannot measure are pinned to the rules that fix them:
 * the 375px onboarding banner clipped its own call to action, the request-list
 * status badge collapsed to an icon below ~1150px, the overview's disclosure
 * table clipped inside its card, the uppercase column headers were 10.5px at
 * 3.15:1, the `ENABLED` badge painted Mantine's own teal ramp at 4.32:1, the
 * probe dialog reported a failure in Mantine's red-6 at 2.86:1, and the banner
 * kept its desktop two-column row all the way down to 375px.
 */
const rawCss = readFileSync(join(process.cwd(), 'src', 'styles.css'), 'utf8')
/** Comments sit above and inside rules; a selector or a declaration must not have to dodge them. */
const css = rawCss.replace(/\/\*[\s\S]*?\*\//g, '')
const source = (name: string) => readFileSync(join(process.cwd(), 'src', name), 'utf8')
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, (character) => `\\${character}`)
const rules = (selector: string) => [...css.matchAll(new RegExp(`(?:^|[},])\\s*${escape(selector)}\\s*(?:,[^{]*)?\\{([^}]*)\\}`, 'gm'))].map((match) => match[1])
/**
 * One rule inside a media block, e.g. `@media (max-width: 40em)`. The viewport
 * rules are where a responsive fix lives, and they are not reachable through
 * `rules()`, which only walks top-level declarations.
 */
const mediaRules = (query: string, selector: string) => {
  const start = css.indexOf(`@media ${query}`)
  if (start < 0) return []
  const next = css.indexOf('@media', start + 1)
  const block = css.slice(start, next < 0 ? css.length : next)
  return [...block.matchAll(new RegExp(`${escape(selector)}\\s*\\{([^}]*)\\}`, 'g'))].map((match) => match[1])
}
const declarations = (bodies: string[], property: string) => bodies.flatMap((body) => [...body.matchAll(new RegExp(`(?:^|;)\\s*${property}\\s*:\\s*([^;]+)`, 'g'))].map((match) => match[1].trim()))
const declaration = (selector: string, property: string) => declarations(rules(selector), property)

/** Relative luminance and the WCAG contrast ratio, so the header colour is judged, not trusted. */
const luminance = (hex: string) => {
  const channels = [1, 3, 5].map((index) => parseInt(hex.slice(index, index + 2), 16) / 255)
  const [red, green, blue] = channels.map((channel) => (channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4))
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue
}
const contrast = (foreground: string, background: string) => {
  const [lighter, darker] = [luminance(foreground), luminance(background)].sort((left, right) => right - left)
  return (lighter + 0.05) / (darker + 0.05)
}
const mix = (top: string, bottom: string, weight: number) => `#${[1, 3, 5]
  .map((index) => Math.round(parseInt(top.slice(index, index + 2), 16) * weight + parseInt(bottom.slice(index, index + 2), 16) * (1 - weight)).toString(16).padStart(2, '0'))
  .join('')}`
const token = (name: string) => css.match(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})`, 'i'))?.[1] as string

describe('the onboarding banner keeps its call to action whole', () => {
  it('never lets the banner action be the flexible item', () => {
    // Mantine's button is `overflow: hidden`, so a squeezed flex line clipped its
    // own label: at 375px "配置渠道" rendered as "配置渠".
    expect(declaration('.pm-onboarding-row > :last-child', 'flex')).toEqual(['0 0 auto'])
    expect(declaration('.pm-onboarding-row > :first-child', 'flex')).toEqual(['1 1 200px'])
    expect(declaration('.pm-onboarding-row > :first-child', 'min-width')).toEqual(['0'])
  })

  it('marks the row the rule targets', () => {
    expect(source('Shell.tsx')).toContain('pm-onboarding-row')
  })
})

describe('a request status code stays readable', () => {
  it('renders the code next to its icon', () => {
    render(<Status code={502} />)
    expect(screen.getByText('502')).toBeInTheDocument()
  })

  it('never shrinks the badge below its own content', () => {
    // `width: fit-content` plus `overflow: hidden` let a squeezed table column
    // collapse the label to clientWidth 0, leaving only the ✓/⚠ icon.
    render(<Status code={200} />)
    expect(document.querySelector('.status-badge')).not.toBeNull()
    expect(declaration('.status-badge', 'min-width')).toEqual(['max-content'])
  })
})

describe('the overview disclosure table scrolls inside its card', () => {
  it('scrolls instead of being clipped by the card', () => {
    // 352px of table inside a 305px <details> inside an `overflow: hidden` card
    // cut the third header to "ERROR" at 375px in English.
    expect(declaration('.chart-table', 'overflow-x')).toEqual(['auto'])
  })
})

describe('column headers clear AA at their own size', () => {
  it('is at least 12px', () => {
    const sizes = declaration('thead th', 'font-size').map((value) => Number(value.replace('px', '')))
    expect(sizes).toEqual([12])
  })

  it('uses a colour that measures at least 4.5:1 on the header fill, in both modes', () => {
    // The header paints `--surface-muted` at 94% over the surface beneath it.
    expect(declaration('thead th', 'color')).toEqual(['var(--muted)'])
    const light = contrast(token('muted'), mix(token('surface-muted'), token('surface'), 0.94))
    // The raw ratio is compared; formatting stays in the message, because a value
    // that only passes once it is rounded to two decimals is below AA.
    expect(light, `light header = ${light.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
    // The dark block redeclares both tokens, so the ratio is checked on the pair
    // the dark-mode header actually resolves.
    const darkBlock = css.slice(css.indexOf(":root[data-mode='dark']"))
    const darkToken = (name: string) => darkBlock.match(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})`, 'i'))?.[1] as string
    const dark = contrast(darkToken('muted'), mix(darkToken('surface-muted'), darkToken('surface'), 0.94))
    expect(dark, `dark header = ${dark.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
  })
})

/**
 * axe reported `aria-prohibited-attr` on `.chart-wrap`: the trend chart is a plain
 * `div` carrying `aria-label`, and a generic element has no role that permits a
 * name, so the label was dropped from the accessibility tree entirely — the same
 * class as the loading stack, which was fixed with `role="status"`.
 */
describe('the trend chart carries a name a screen reader can reach', () => {
  it('gives the labelled chart a role that permits its name', () => {
    const overview = source('pages/OverviewPage.tsx')
    const chart = overview.match(/<div[^>]*className="chart-wrap"[^>]*>/)?.[0] as string
    expect(chart).toBeTruthy()
    expect(chart).toMatch(/role="img"/)
    expect(chart).toMatch(/aria-label=/)
  })
})

/** The variables a palette resolves for one scheme — what Mantine actually paints from. */
const resolverVariables = (palette: string) =>
  (buildResolver(palette) as unknown as (theme: unknown) => Record<'light' | 'dark', Record<string, string>>)(undefined)

/**
 * Mantine's `variant="light"` badge paints `--mantine-color-{name}-light` behind
 * `--mantine-color-{name}-light-color`, and only the `pangolin` family was mapped
 * onto the palette: every status family kept Mantine's own ramp. axe 4.12.1
 * measured the Models/Channels `ENABLED` badge at 4.32:1 on the 2026-09-22 release
 * binary — Mantine's teal-9 `#087f5b` on teal-1 `#c3fae8` — below AA for its 11px
 * bold label. The same pair is reachable from `HealthPill` (teal for a row whose
 * consecutive-failure count is 0), whose warning branch measured 2.69:1
 * (`#e67700` on `#fff3bf`) and whose sibling in the probes table, the success
 * badge, measured 3.81:1 (`#2b8a3e` on `#d3f9d8`). Status ink therefore comes from
 * the palette, whose guard already holds every status colour at 4.5:1 on its own
 * soft fill and on the canvas.
 */
const STATUS_FAMILIES: Record<string, [keyof PaletteTokens, keyof PaletteTokens]> = {
  teal: ['success', 'successSoft'],
  green: ['success', 'successSoft'],
  yellow: ['warning', 'warningSoft'],
  red: ['danger', 'dangerSoft'],
}

describe('status pills paint palette ink instead of Mantine’s default ramp', () => {
  it('records the ramp pair axe measured below AA', () => {
    expect(contrast('#087f5b', '#c3fae8')).toBeLessThan(4.5)
  })

  for (const palette of ['bronze', 'slate', 'jade']) {
    for (const mode of ['light', 'dark'] as const) {
      it(`maps every status family onto palette tokens that clear 4.5:1 (${palette}/${mode})`, () => {
        const variables = resolverVariables(palette)[mode]
        const tokens = resolvePalette(palette, mode)
        for (const [family, [foreground, background]] of Object.entries(STATUS_FAMILIES)) {
          expect(variables[`--mantine-color-${family}-light`], `${family} fill`).toBe(tokens[background])
          expect(variables[`--mantine-color-${family}-light-color`], `${family} ink`).toBe(tokens[foreground])
          const ratio = contrast(variables[`--mantine-color-${family}-light-color`], variables[`--mantine-color-${family}-light`])
          // The raw ratio is what has to clear the bar: a value that only passes
          // after being rounded to two decimals is below AA. Formatting belongs in
          // the message, where it makes a failure readable.
          expect(ratio, `${family} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
        }
      })
    }
  }

  it('is the pair the ENABLED badge consumes', () => {
    render(<MantineProvider theme={buildTheme('house', 'bronze', 'light')} env="test"><EnabledPill enabled={1} /></MantineProvider>)
    const badge = document.querySelector('.mantine-Badge-root')
    expect(badge?.getAttribute('style')).toContain('--badge-bg: var(--mantine-color-teal-light)')
    expect(badge?.getAttribute('style')).toContain('--badge-color: var(--mantine-color-teal-light-color)')
  })
})

/**
 * The probe dialog reported a failed probe with `<Text c="red">`, which resolves
 * to Mantine's own red-6 `#fa5252`: axe measured 2.86:1 on the light canvas
 * `#f2efe9` at 12.5px. Failure copy takes the semantic danger token instead, which
 * the palette guard holds at 4.5:1 on the canvas in both modes.
 */
describe('failure copy uses the semantic danger ink', () => {
  it('measures Mantine’s red-6 below AA on the canvas', () => {
    expect(contrast('#fa5252', '#f2efe9')).toBeLessThan(4.5)
  })

  it('defines one error-text style on the danger token', () => {
    expect(declaration('.error-text', 'color')).toEqual(['var(--danger)'])
  })

  it('clears 4.5:1 on the canvas in both modes, for every brand palette', () => {
    for (const palette of ['bronze', 'slate', 'jade']) {
      for (const mode of ['light', 'dark'] as const) {
        const tokens = resolvePalette(palette, mode)
        const ratio = contrast(tokens.danger, tokens.canvas)
        expect(ratio, `${palette}/${mode} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
      }
    }
  })

  it('is what the probe dialog’s failure line uses', () => {
    const channels = source('pages/ChannelsPage.tsx')
    expect(channels).toContain('className="error-text"')
    expect(channels).not.toContain('c="red"')
  })
})

/** `color-mix(in srgb, #a 55%, #b)` composited, so a hover fill can be measured. */
const tintedFill = (value: string) => {
  const match = value.match(/^color-mix\(in srgb, (#[0-9a-f]{6}) (\d+)%, (#[0-9a-f]{6})\)$/i)
  return match ? mix(match[1], match[3], Number(match[2]) / 100) : null
}

/**
 * `InlineQueryError` puts a `variant="outline" color="red"` retry button on its own
 * `--danger-soft` alert. Mantine's outline variant reads the label *and* the border
 * from `--mantine-color-red-outline` (red-6 `#fa5252`) and its hover fill from
 * `-outline-hover`, so the label measured ~3.0:1 against the alert's own fill —
 * the residual risk recorded in the B1 report, now reproduced. The retry stays a
 * red action and takes the palette's danger pair; `filled` controls are untouched.
 */
describe('the inline query error retry reads at AA', () => {
  const outlineVariables = ['--mantine-color-red-outline', '--mantine-color-red-text']

  it('measures Mantine’s red-6 below AA on the alert fill', () => {
    expect(contrast('#fa5252', '#f9e6e3')).toBeLessThan(4.5)
  })

  it('is the variable the retry button consumes for its label and hover', () => {
    render(<MantineProvider theme={buildTheme('house', 'bronze', 'light')} env="test"><InlineQueryError message="Upstream timed out" onRetry={() => {}} /></MantineProvider>)
    const style = document.querySelector('.mantine-Button-root')?.getAttribute('style') ?? ''
    expect(style).toContain('--button-color: var(--mantine-color-red-outline)')
    expect(style).toContain('solid var(--mantine-color-red-outline)')
    expect(style).toContain('--button-hover: var(--mantine-color-red-outline-hover)')
  })

  for (const palette of ['bronze', 'slate', 'jade']) {
    for (const mode of ['light', 'dark'] as const) {
      it(`resolves the red outline ink from the palette and clears 4.5:1 (${palette}/${mode})`, () => {
        const variables = resolverVariables(palette)[mode]
        const tokens = resolvePalette(palette, mode)
        for (const name of outlineVariables) expect(variables[name], name).toBe(tokens.danger)
        // The alert's own fill is the background the retry sits on.
        const onAlert = contrast(tokens.danger, tokens.dangerSoft)
        expect(onAlert, `danger on danger-soft = ${onAlert.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
        // And the hover fill must not drop the label under AA either.
        const hoverFill = tintedFill(variables['--mantine-color-red-outline-hover'])
        if (!hoverFill) throw new Error(`outline hover fill is not a measurable tint: ${variables['--mantine-color-red-outline-hover']}`)
        const onHover = contrast(tokens.danger, hoverFill)
        expect(onHover, `danger on ${hoverFill} = ${onHover.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
      })
    }
  }
})

/**
 * The banner kept its desktop two-column row at every width: the row is `nowrap`
 * so the action holds its intrinsic width, and at 375px that left the explanation
 * a ~150px column that broke almost word by word (`probes-en-light-375.png` in
 * the controller's evidence). Below the mobile breakpoint the copy takes the full
 * line and the one action plus the dismiss control share the line under it.
 */
describe('the onboarding banner stacks below the mobile breakpoint', () => {
  it('gives the explanation the full line', () => {
    expect(declarations(mediaRules('(max-width: 40em)', '.pm-onboarding .pm-onboarding-row'), 'flex-wrap')).toEqual(['wrap'])
    expect(declarations(mediaRules('(max-width: 40em)', '.pm-onboarding .pm-onboarding-row > :first-child'), 'flex-basis')).toEqual(['100%'])
  })

  it('keeps the desktop row above it', () => {
    expect(declaration('.pm-onboarding-row > :first-child', 'flex')).toEqual(['1 1 200px'])
    expect(declaration('.pm-onboarding-row > :last-child', 'flex')).toEqual(['0 0 auto'])
  })
})
