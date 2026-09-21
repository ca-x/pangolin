import { Button, MantineProvider } from '@mantine/core'
import { cleanup, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { buildTheme } from './mantine'
import { palettes } from './palettes'

const modes = ['light', 'dark'] as const

function renderButton(palette: string, mode: (typeof modes)[number], label = 'Apply') {
  return render(
    <MantineProvider theme={buildTheme('house', palette, mode)} env="test">
      <Button>{label}</Button>
    </MantineProvider>,
  )
}

// Mantine resolves a filled button's label from `theme.white` and writes it as an
// inline custom property, so the theme's vars channel is the only place a fix can
// land. Rendering through the real theme is what catches a broken vars shape: an
// object where Mantine calls a function threw on every button and blanked the
// console, and the mocked providers in the other suites never noticed.
describe('filled button label', () => {
  it('takes the accent contrast from the theme', () => {
    renderButton('bronze', 'dark')
    const button = screen.getByRole('button', { name: 'Apply' })
    expect(button.getAttribute('style')).toContain('--button-color: var(--accent-contrast)')
    expect(button.getAttribute('style')).not.toContain('--mantine-color-white')
  })

  for (const palette of palettes) {
    for (const mode of modes) {
      it(`renders for ${palette.id} (${mode})`, () => {
        expect(() => renderButton(palette.id, mode)).not.toThrow()
        cleanup()
      })
    }
  }
})
