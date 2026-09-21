import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { skinVariables } from './mantine'
import { palettes, paletteById } from './palettes'
import { resolveSkin, skins } from './skins'

export type ColorMode = 'system' | 'light' | 'dark'
/** Kept for the palettes the product shipped with; they are ordinary palettes now. */
export type Accent = string

type ThemeState = {
  mode: ColorMode
  skin: string
  palette: string
  setMode: (mode: ColorMode) => void
  /** Applying a skin also applies its recommended palette and mode. */
  setSkin: (skin: string) => void
  setPalette: (palette: string) => void
}
const ThemeContext = createContext<ThemeState | null>(null)

const stored = (key: string, fallback: string) => {
  const value = typeof localStorage === 'undefined' ? null : localStorage.getItem(key)
  return value ?? fallback
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<ColorMode>(() => stored('pangolin-color-mode', 'system') as ColorMode)
  const [skin, setSkinState] = useState<string>(() => {
    const value = stored('pangolin-skin', 'house')
    return skins.some((entry) => entry.id === value) ? value : 'house'
  })
  const [palette, setPaletteState] = useState<string>(() => {
    const value = stored('pangolin-palette', stored('pangolin-accent', 'bronze'))
    return paletteById.has(value) ? value : palettes[0].id
  })

  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      const resolved = mode === 'system' ? (media.matches ? 'dark' : 'light') : mode
      const root = document.documentElement
      root.dataset.mode = resolved
      root.dataset.skin = skin
      root.dataset.material = resolveSkin(skin).material
      root.dataset.motion = resolveSkin(skin).motion
      root.dataset.density = resolveSkin(skin).density
      for (const [property, value] of Object.entries(skinVariables(skin, palette, resolved))) {
        root.style.setProperty(property, value)
      }
      document.querySelector('meta[name="theme-color"]')?.setAttribute('content', resolved === 'dark' ? '#151312' : '#f4f2ee')
    }
    apply()
    media.addEventListener('change', apply)
    localStorage.setItem('pangolin-color-mode', mode)
    localStorage.setItem('pangolin-skin', skin)
    localStorage.setItem('pangolin-palette', palette)
    return () => media.removeEventListener('change', apply)
  }, [mode, skin, palette])

  const value = useMemo<ThemeState>(() => ({
    mode,
    skin,
    palette,
    setMode,
    setSkin: (next: string) => {
      const skin = resolveSkin(next)
      setSkinState(skin.id)
      if (paletteById.has(skin.recommendedPalette)) setPaletteState(skin.recommendedPalette)
      if (skin.preferredMode !== 'auto') setMode(skin.preferredMode)
    },
    setPalette: setPaletteState,
  }), [mode, skin, palette])
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme() {
  const value = useContext(ThemeContext)
  if (!value) throw new Error('useTheme must be used inside ThemeProvider')
  return value
}

/** The mode actually in effect once `system` is resolved against the OS preference. */
export function useResolvedMode() {
  const { mode } = useTheme()
  const [resolved, setResolved] = useState<'light' | 'dark'>('light')
  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)')
    const apply = () => setResolved(mode === 'system' ? (media.matches ? 'dark' : 'light') : mode)
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [mode])
  return resolved
}
