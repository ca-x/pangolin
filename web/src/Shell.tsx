import { Activity, Boxes, Gauge, KeyRound, Menu, Settings, Unplug, X } from 'lucide-react'
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink, Outlet, useNavigate } from 'react-router'
import { api } from './api'

export default function Shell() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [mobileOpen, setMobileOpen] = useState(false)
  const menuButton = useRef<HTMLButtonElement>(null)
  const closeButton = useRef<HTMLButtonElement>(null)
  const menuWasOpened = useRef(false)
  const links = [
    { to: '/', label: t('overview'), icon: Gauge, end: true },
    { to: '/providers', label: t('providers'), icon: Unplug },
    { to: '/models', label: t('models'), icon: Boxes },
    { to: '/keys', label: t('apiKeys'), icon: KeyRound },
    { to: '/requests', label: t('requests'), icon: Activity },
    { to: '/settings', label: t('settings'), icon: Settings },
  ]
  useEffect(() => {
    if (mobileOpen) {
      menuWasOpened.current = true
      closeButton.current?.focus()
    } else if (menuWasOpened.current) {
      menuButton.current?.focus()
    }
  }, [mobileOpen])
  const trapNavigationFocus = (event: KeyboardEvent<HTMLElement>) => {
    if (!mobileOpen) return
    if (event.key === 'Escape') { setMobileOpen(false); return }
    if (event.key !== 'Tab') return
    const focusable = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('a[href], button:not([disabled])')).filter((element) => element.offsetParent !== null)
    const first = focusable[0]; const last = focusable.at(-1)
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus() }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
  }
  const signOut = async () => { await api('/api/v1/auth/logout', { method: 'POST' }); navigate('/login'); location.reload() }
  return <div className="app-shell">
    <a className="skip-link" href="#main-content">{t('skipToContent')}</a>
    <header className="mobile-header"><Brand compact /><button ref={menuButton} className="icon-button" onClick={() => setMobileOpen(true)} aria-label={t('menu')}><Menu size={20} /></button></header>
    {mobileOpen && <button className="nav-scrim" onClick={() => setMobileOpen(false)} aria-label={t('close')} />}
    <aside className={`sidebar ${mobileOpen ? 'sidebar-open' : ''}`} role={mobileOpen ? 'dialog' : undefined} aria-modal={mobileOpen || undefined} aria-label={mobileOpen ? t('menu') : undefined} onKeyDown={trapNavigationFocus}>
      <div className="sidebar-top"><Brand /><button ref={closeButton} className="icon-button sidebar-close" onClick={() => setMobileOpen(false)} aria-label={t('close')}><X size={18} /></button></div>
      <nav aria-label="Primary">{links.map(({ to, label, icon: Icon, end }) => <NavLink onClick={() => setMobileOpen(false)} className={({ isActive }) => isActive ? 'nav-link nav-link-active' : 'nav-link'} key={to} to={to} end={end}><Icon size={18} strokeWidth={1.8} /><span>{label}</span></NavLink>)}</nav>
      <button className="signout" onClick={signOut}>{t('signOut')}</button>
    </aside>
    <main id="main-content" className="content" tabIndex={-1} inert={mobileOpen || undefined} aria-hidden={mobileOpen || undefined}><Outlet /></main>
  </div>
}

function Brand({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation()
  return <div className={`brand ${compact ? 'brand-compact' : ''}`}><img src="/logo.webp" alt="" /><div><strong>{t('product')}</strong><span>{t('productSubtitle')}</span></div></div>
}
