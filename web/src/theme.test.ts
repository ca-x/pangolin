import { describe, expect, it } from 'vitest'
import { buildResolver, buildTheme } from './mantine'
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
