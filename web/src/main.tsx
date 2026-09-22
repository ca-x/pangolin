import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
// Mantine first, inside CSS layers, so our unlayered styles.css keeps precedence.
import '@mantine/core/styles.layer.css'
import { MantineProvider } from '@mantine/core'
import * as Tooltip from '@radix-ui/react-tooltip'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { StrictMode, useMemo, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import App from './App'
import { ConfirmHost } from './components'
import './i18n'
import './styles.css'
import './materials.css'
import { buildResolver, buildTheme } from './mantine'
import { resolveSkin } from './skins'
import { ThemeProvider, useResolvedMode, useTheme } from './theme'
import { ThemedToaster } from './toaster'

const queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 5_000, retry: 1 } } })

/** Rebuilds the Mantine theme whenever the skin, palette or resolved mode changes. */
function MantineShell({ children }: { children: ReactNode }) {
  const { skin, palette } = useTheme()
  const resolved = useResolvedMode()
  const theme = useMemo(() => buildTheme(skin, palette, resolved), [skin, palette, resolved])
  const resolver = useMemo(() => buildResolver(palette, Boolean(resolveSkin(skin).highContrast)), [palette, skin])
  return <MantineProvider theme={theme} cssVariablesResolver={resolver} forceColorScheme={resolved}>{children}</MantineProvider>
}

createRoot(document.getElementById('root')!).render(<StrictMode><QueryClientProvider client={queryClient}><ThemeProvider><MantineShell><BrowserRouter>{/* Radix tooltip provider is removed once `Tip` moves to Mantine. */}<Tooltip.Provider delayDuration={400} skipDelayDuration={300}><App/><ConfirmHost/><ThemedToaster/></Tooltip.Provider></BrowserRouter></MantineShell></ThemeProvider></QueryClientProvider></StrictMode>)
