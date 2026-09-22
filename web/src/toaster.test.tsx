import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'
import i18n from './i18n'
import { ThemeProvider } from './theme'
import { ThemedToaster } from './toaster'

/**
 * Sonner names its own region "Notifications alt+T" in English, whatever the
 * console's language is — an untranslated accessible name on every screen, since
 * the toast host is mounted at the application root.
 */
describe('the toast region is named in the active language', () => {
  beforeEach(() => { void i18n.changeLanguage('zh-CN') })

  it('uses the localized name instead of the library default', async () => {
    render(<ThemeProvider><ThemedToaster /></ThemeProvider>)
    expect(screen.getByLabelText(/通知/)).toBeInTheDocument()
    expect(screen.queryByLabelText(/^Notifications/)).not.toBeInTheDocument()
    await i18n.changeLanguage('en')
    expect(screen.getByLabelText(/^Notifications/)).toBeInTheDocument()
    await i18n.changeLanguage('zh-CN')
  })
})
