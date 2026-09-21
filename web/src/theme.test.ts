import { describe, expect, it } from 'vitest'
import { buildResolver, buildTheme } from './mantine'
import { contrastRatio } from './color'
import { palettes, resolvePalette } from './palettes'

const modes = ['light', 'dark'] as const

// The palette audit checks the accent contrast *token*. These check that the
// token actually reaches the components: Mantine's filled button hard-codes a
// white label, which measured 2.61:1 on the bronze accent in dark mode.
describe('filled controls', () => {
  for (const palette of palettes) {
    for (const mode of modes) {
      it(`labels ${palette.id} (${mode}) with its accent contrast`, () => {
        const resolver = buildResolver(palette.id)({} as never)
        const variables = (mode === 'light' ? resolver.light : resolver.dark) as Record<string, string>
        expect(variables['--mantine-primary-color-contrast']).toBe(resolvePalette(palette.id, mode).accentContrast)
      })
    }
  }

  it('maps the by-name colour variables onto the palette, not the ramp extremes', () => {
    // Mantine's light/default/outline variants resolve `--mantine-color-pangolin-*`
    // by name. Left to the generated ramp, a light button measured near-white text
    // on a near-black surface — legible, but not a button.
    const resolver = buildResolver('bronze')({} as never)
    for (const [mode, variables] of [['light', resolver.light], ['dark', resolver.dark]] as const) {
      const tokens = resolvePalette('bronze', mode)
      const vars = variables as Record<string, string>
      expect(vars['--mantine-color-pangolin-light'], mode).toBe(tokens.accentSoft)
      expect(vars['--mantine-color-pangolin-light-color'], mode).toBe(tokens.accentStrong)
      // A hover that equals the resting state is a variant that ignores the
      // pointer, which is how the first version of this mapping regressed.
      expect(vars['--mantine-color-pangolin-light-hover'], mode).not.toBe(vars['--mantine-color-pangolin-light'])
      expect(vars['--mantine-color-pangolin-outline-hover'], mode).not.toBe(vars['--mantine-color-pangolin-outline'])
      expect(vars['--mantine-color-pangolin-filled'], mode).toBe(tokens.accent)
      expect(vars['--mantine-color-pangolin-contrast'], mode).toBe(tokens.accentContrast)
      // The light variant must stay readable, which is what made the old pair wrong.
      expect(contrastRatio(tokens.accentStrong, tokens.accentSoft), mode).toBeGreaterThanOrEqual(4.5)
    }
  })

  it('gives every button the class the material layer hooks onto', () => {
    const theme = buildTheme('house', 'bronze', 'dark')
    expect(theme.components?.Button?.defaultProps?.className).toContain('pm-button')
  })

  it('points the filled button label at the accent contrast', () => {
    // Mantine writes `--button-color` inline from `theme.white`, so this has to
    // go through theme vars, which are merged after the component's own resolver.
    // It must be a function: Mantine calls it, and passing an object threw during
    // render and blanked the console.
    const theme = buildTheme('house', 'bronze', 'dark')
    const vars = theme.components?.Button?.vars as (
      theme: unknown,
      props: { variant?: string },
    ) => Record<string, Record<string, string>>
    expect(typeof vars).toBe('function')
    // No variant means Mantine's default, which is filled.
    expect(vars({}, {}).root['--button-color']).toBe('var(--accent-contrast)')
    expect(vars({}, { variant: 'filled' }).root['--button-color']).toBe('var(--accent-contrast)')
    // Every other variant draws its text from the same property, so overriding
    // them put dark ink on a dark surface (measured 1.05:1 in the browser).
    for (const variant of ['default', 'light', 'outline', 'subtle', 'transparent']) {
      expect(vars({}, { variant }).root, variant).toBeUndefined()
    }
  })
})
