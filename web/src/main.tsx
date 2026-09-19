import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import { Toaster } from 'sonner'
import App from './App'
import './i18n'
import './styles.css'
import { ThemeProvider } from './theme'

const queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 5_000, retry: 1 } } })

createRoot(document.getElementById('root')!).render(<StrictMode><QueryClientProvider client={queryClient}><ThemeProvider><BrowserRouter><App/><Toaster richColors position="bottom-right"/></BrowserRouter></ThemeProvider></QueryClientProvider></StrictMode>)
