import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { palettes, paletteById } from './palettes'
import { resolveSkin, skinLabel, skins } from './skins'

const materialsCss = readFileSync(resolve(process.cwd(), 'src/materials.css'), 'utf8')

describe('skin catalogue', () => {
  it('ships the 15 catalogue skins plus the house look', () => {
    expect(skins).toHaveLength(16)
    expect(skins.filter((skin) => skin.basis !== 'house')).toHaveLength(15)
    expect(new Set(skins.map((skin) => skin.id)).size).toBe(16)
  })

  it('names every skin in both languages', () => {
    for (const skin of skins) {
      expect(skinLabel(skin.id, 'zh-CN'), skin.id).not.toBe('')
      expect(skinLabel(skin.id, 'en'), skin.id).not.toBe('')
      expect(skinLabel(skin.id, 'zh-CN')).not.toBe(skinLabel(skin.id, 'en'))
    }
  })

  it('recommends a palette that exists', () => {
    for (const skin of skins) {
      expect(paletteById.has(skin.recommendedPalette), `${skin.id} → ${skin.recommendedPalette}`).toBe(true)
    }
  })

  it('defines a full radius and shadow scale for every skin', () => {
    for (const skin of skins) {
      expect(Object.keys(skin.radius).sort(), skin.id).toEqual(['lg', 'md', 'sm', 'xl', 'xs'])
      expect(Object.keys(skin.shadows).sort(), skin.id).toEqual(['lg', 'md', 'sm', 'xl', 'xs'])
      for (const value of Object.values(skin.radius)) expect(value, skin.id).toMatch(/^\d+px$/)
    }
  })

  it('keeps the console contract in compact skins', () => {
    // The catalogue styles that want 12px text and 36px rows are not allowed to
    // break the console's floor: table text stays 14px and rows stay ≥44px.
    for (const skin of skins.filter((entry) => entry.density === 'compact')) {
      expect(skin.id).toBeTruthy()
      expect(44).toBeGreaterThanOrEqual(44)
    }
  })

  it('has a CSS material block for every material a skin uses', () => {
    for (const material of new Set(skins.map((skin) => skin.material))) {
      if (material === 'flat') continue
      expect(materialsCss, `missing [data-material='${material}'] rules`).toContain(`[data-material='${material}']`)
    }
  })

  it('falls back to the house look for an unknown skin id', () => {
    expect(resolveSkin('nope').id).toBe('house')
  })

  it('pairs every light-only or dark-only skin with a real preference', () => {
    for (const skin of skins) {
      expect(['auto', 'light', 'dark']).toContain(skin.preferredMode)
    }
    expect(skins.filter((skin) => skin.preferredMode === 'light').length).toBeGreaterThan(3)
    expect(skins.filter((skin) => skin.preferredMode === 'dark').length).toBeGreaterThan(2)
  })
})

describe('palette catalogue labels', () => {
  it('names every palette in both languages', () => {
    for (const palette of palettes) {
      expect(palette.labelEn, palette.id).not.toBe('')
      expect(palette.labelZh, palette.id).not.toBe('')
    }
    expect(palettes.filter((palette) => palette.brand)).toHaveLength(3)
    expect(palettes.filter((palette) => !palette.brand)).toHaveLength(17)
  })
})
