import { Checkbox, MantineProvider, Pagination } from '@mantine/core'
import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { buildTheme } from './mantine'
import { palettes } from './palettes'

/**
 * Filled buttons were fixed once already (`filledButton.test.tsx`), but the
 * accent-filled controls around them kept Mantine's white ink: in dark mode the
 * active page number measured 2.61:1 on the light bronze accent, and the same
 * white is what a checked box paints its tick in. Every control that paints on
 * the accent fill takes the palette's own contrast ink instead.
 */
function renderIn(ui: React.ReactElement, palette = 'bronze', mode: 'light' | 'dark' = 'dark') {
  return render(<MantineProvider theme={buildTheme('house', palette, mode)} env="test">{ui}</MantineProvider>)
}

describe('controls painted on the accent fill', () => {
  it('gives the active pagination control the accent contrast', () => {
    renderIn(<Pagination total={5} value={3} onChange={() => {}} />)
    const active = document.querySelector('[data-active]')
    expect(active).not.toBeNull()
    // The variable is declared on the pagination root and inherited by the active
    // control, which Mantine otherwise paints white.
    const root = document.querySelector('.mantine-Pagination-root')
    expect(root?.getAttribute('style')).toContain('--pagination-active-color: var(--accent-contrast)')
    expect(root?.getAttribute('style')).not.toContain('--mantine-color-white')
  })

  it('gives a checked box the accent contrast tick', () => {
    renderIn(<Checkbox defaultChecked label="Enabled" />)
    const root = document.querySelector('.mantine-Checkbox-root')
    expect(root?.getAttribute('style')).toContain('--checkbox-icon-color: var(--accent-contrast)')
  })

  for (const palette of palettes) {
    for (const mode of ['light', 'dark'] as const) {
      it(`renders the pagination control for ${palette.id} (${mode})`, () => {
        expect(() => renderIn(<Pagination total={3} value={1} onChange={() => {}} />, palette.id, mode)).not.toThrow()
      })
    }
  }
})
