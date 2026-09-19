import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'

export type ColorMode = 'system' | 'light' | 'dark'
export type Accent = 'bronze' | 'slate' | 'jade'

type ThemeState = { mode: ColorMode; accent: Accent; setMode: (mode: ColorMode) => void; setAccent: (accent: Accent) => void }
const ThemeContext = createContext<ThemeState | null>(null)

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<ColorMode>(() => (localStorage.getItem('pangolin-color-mode') as ColorMode) || 'system')
  const [accent, setAccent] = useState<Accent>(() => (localStorage.getItem('pangolin-accent') as Accent) || 'bronze')

  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      const resolved = mode === 'system' ? (media.matches ? 'dark' : 'light') : mode
      document.documentElement.dataset.mode = resolved
      document.documentElement.dataset.accent = accent
      document.querySelector('meta[name="theme-color"]')?.setAttribute('content', resolved === 'dark' ? '#151312' : '#f4f2ee')
    }
    apply()
    media.addEventListener('change', apply)
    localStorage.setItem('pangolin-color-mode', mode)
    localStorage.setItem('pangolin-accent', accent)
    return () => media.removeEventListener('change', apply)
  }, [mode, accent])

  const value = useMemo(() => ({ mode, accent, setMode, setAccent }), [mode, accent])
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}
export function useTheme() {
  const value = useContext(ThemeContext)
  if (!value) throw new Error('useTheme must be used inside ThemeProvider')
  return value
}
