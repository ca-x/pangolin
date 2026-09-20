import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
import * as Tooltip from '@radix-ui/react-tooltip'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useEffect, useState, StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import { Toaster } from 'sonner'
import App from './App'
import './i18n'
import './styles.css'
import { ThemeProvider, useTheme } from './theme'

const queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 5_000, retry: 1 } } })

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

createRoot(document.getElementById('root')!).render(<StrictMode><QueryClientProvider client={queryClient}><ThemeProvider><BrowserRouter><Tooltip.Provider delayDuration={400} skipDelayDuration={300}><App/><ThemedToaster/></Tooltip.Provider></BrowserRouter></ThemeProvider></QueryClientProvider></StrictMode>)
