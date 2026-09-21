import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
// Mantine first, inside CSS layers, so our unlayered styles.css keeps precedence.
import '@mantine/core/styles.layer.css'
import { MantineProvider } from '@mantine/core'
import * as Tooltip from '@radix-ui/react-tooltip'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { StrictMode, useEffect, useMemo, useState, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import { Toaster } from 'sonner'
import App from './App'
import './i18n'
import './styles.css'
import './materials.css'
import { buildResolver, buildTheme } from './mantine'
import { resolveSkin } from './skins'
import { ThemeProvider, useResolvedMode, useTheme } from './theme'

const queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 5_000, retry: 1 } } })

/** Rebuilds the Mantine theme whenever the skin, palette or resolved mode changes. */
function MantineShell({ children }: { children: ReactNode }) {
  const { skin, palette } = useTheme()
  const resolved = useResolvedMode()
  const theme = useMemo(() => buildTheme(skin, palette, resolved), [skin, palette, resolved])
  const resolver = useMemo(() => buildResolver(palette, Boolean(resolveSkin(skin).highContrast)), [palette, skin])
  return <MantineProvider theme={theme} cssVariablesResolver={resolver} forceColorScheme={resolved}>{children}</MantineProvider>
}

function ThemedToaster() {
  const { mode } = useTheme()
  const [theme, setTheme] = useState<'light' | 'dark'>('light')
  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)')
    const apply = () => setTheme(mode === 'system' ? (media.matches ? 'dark' : 'light') : mode)
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [mode])
  return <Toaster theme={theme} richColors position="bottom-right" toastOptions={{ classNames: { title: 'sonner-title', description: 'sonner-description' } }} />
}

createRoot(document.getElementById('root')!).render(<StrictMode><QueryClientProvider client={queryClient}><ThemeProvider><MantineShell><BrowserRouter>{/* Radix tooltip provider is removed once `Tip` moves to Mantine. */}<Tooltip.Provider delayDuration={400} skipDelayDuration={300}><App/><ThemedToaster/></Tooltip.Provider></BrowserRouter></MantineShell></ThemeProvider></QueryClientProvider></StrictMode>)
