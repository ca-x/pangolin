import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Toaster } from 'sonner'
import { useTheme } from './theme'

/**
 * The toast host. Sonner's own container name is the hard-coded English
 * `Notifications alt+T`; a region's accessible name is copy like any other, so it
 * is localized here. The theme follows the resolved colour mode rather than the
 * OS preference alone, which is what keeps a light toast off a dark console.
 */
export function ThemedToaster() {
  const { t } = useTranslation()
  const { mode } = useTheme()
  const [theme, setTheme] = useState<'light' | 'dark'>('light')
  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)')
    const apply = () => setTheme(mode === 'system' ? (media.matches ? 'dark' : 'light') : mode)
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [mode])
  return <Toaster theme={theme} richColors position="bottom-right" containerAriaLabel={t('notifications')} toastOptions={{ classNames: { title: 'sonner-title', description: 'sonner-description' } }} />
}
