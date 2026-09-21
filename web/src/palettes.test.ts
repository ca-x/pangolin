import { describe, expect, it } from 'vitest'
import { contrastRatio } from './color'
import { palettes, resolvePalette, tokenToVariable, type PaletteTokens } from './palettes'

const modes = ['light', 'dark'] as const

// Bars: 4.5:1 is WCAG AA for normal text, 3:1 for non-text UI and large text.
// faintInk carries micro labels and sits deliberately close to the background,
// so it is held to 3:1 rather than 4.5:1.
const textPairs: Array<[keyof PaletteTokens, keyof PaletteTokens, number]> = [
  ['ink', 'canvas', 4.5],
  ['ink', 'surface', 4.5],
  ['ink', 'surfaceMuted', 4.5],
  ['mutedInk', 'canvas', 4.5],
  ['mutedInk', 'surface', 4.5],
  ['faintInk', 'canvas', 3],
  ['faintInk', 'surface', 3],
  ['success', 'canvas', 4.5],
  ['warning', 'canvas', 4.5],
  ['danger', 'canvas', 4.5],
  ['success', 'surface', 4.5],
  ['danger', 'surface', 4.5],
]

const uiPairs: Array<[keyof PaletteTokens, keyof PaletteTokens, number]> = [
  ['accentContrast', 'accent', 4.5],
  ['controlBorder', 'canvas', 3],
  ['controlBorder', 'surface', 3],
  ['border', 'surface', 1.15],
  ['surface', 'canvas', 1.02],
  ['accent', 'canvas', 3],
  ['accentStrong', 'accentSoft', 4.5],
  ['success', 'successSoft', 4.5],
  ['warning', 'warningSoft', 4.5],
  ['danger', 'dangerSoft', 4.5],
]

describe('palette catalogue', () => {
  it('ships 20 palettes with unique ids', () => {
    expect(palettes).toHaveLength(20)
    expect(new Set(palettes.map((palette) => palette.id)).size).toBe(20)
  })

  it('exposes a CSS variable for every token', () => {
    const tokens = Object.keys(resolvePalette('bronze', 'light')) as Array<keyof PaletteTokens>
    for (const token of tokens) expect(tokenToVariable[token]).toMatch(/^--[a-z-]+$/)
  })

  for (const palette of palettes) {
    for (const mode of modes) {
      describe(`${palette.id} (${mode})`, () => {
        const tokens = resolvePalette(palette.id, mode)

        it('uses only six-digit hex tokens', () => {
          for (const [token, value] of Object.entries(tokens)) {
            expect(value, `${palette.id}/${mode}/${token}`).toMatch(/^#[0-9a-f]{6}$/)
          }
        })

        for (const [foreground, background, minimum] of textPairs) {
          it(`keeps ${foreground} readable on ${background} (≥${minimum}:1)`, () => {
            const ratio = contrastRatio(tokens[foreground], tokens[background])
            expect(ratio, `${palette.id}/${mode} ${foreground} on ${background} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(minimum)
          })
        }

        for (const [foreground, background, minimum] of uiPairs) {
          it(`keeps ${foreground} visible against ${background} (≥${minimum}:1)`, () => {
            const ratio = contrastRatio(tokens[foreground], tokens[background])
            expect(ratio, `${palette.id}/${mode} ${foreground} vs ${background} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(minimum)
          })
        }
      })
    }
  }
})
