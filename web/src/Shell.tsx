import { Activity, Boxes, FlaskConical, Gauge, KeyRound, LogOut, Menu, MessageSquareText, Settings, Unplug, X } from 'lucide-react'
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink, Outlet, useNavigate } from 'react-router'
import { api } from './api'
import type { Branding, User } from './api'
import { ProjectProvider, useProject } from './project'

export default function Shell({ user,branding }: { user: User; branding: Branding }) {
  return <ProjectProvider><ShellContent user={user} branding={branding}/></ProjectProvider>
}

function ShellContent({ user,branding }: { user: User; branding: Branding }) {
  const { t } = useTranslation()
  const { project, projects, permissions, setProjectId } = useProject()
  const navigate = useNavigate()
  const [mobileOpen, setMobileOpen] = useState(false)
  const menuButton = useRef<HTMLButtonElement>(null)
  const closeButton = useRef<HTMLButtonElement>(null)
  const menuWasOpened = useRef(false)
  const can = (permission: string) => permissions.has('*') || permissions.has(permission)
  const canAccess = ['api_key:manage','role:manage','user:manage','oidc:manage'].some(can)
  const groups = [
    { label: t('workspace'), links: [{ to: '/', label: t('overview'), icon: Gauge, end: true },...(can('project:manage')?[{ to: '/channels', label: t('channels'), icon: Unplug },{ to: '/models', label: t('models'), icon: Boxes },{ to: '/prompts', label: t('prompts'), icon: MessageSquareText }]:[]),{ to: '/playground', label: t('playground'), icon: FlaskConical }] },
    { label: t('observe'), links: [{ to: '/operations', label: t('operations'), icon: Activity }] },
    ...((canAccess||can('catalog:manage')) ? [{ label: t('administration'), links: [...(canAccess?[{ to: '/access', label: t('access'), icon: KeyRound }]:[]),...(can('catalog:manage')?[{ to: '/system', label: t('systemSettings'), icon: Settings }]:[])] }] : []),
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
    <header className="mobile-header"><Brand compact name={branding.branding_name}/><button ref={menuButton} className="icon-button" onClick={() => setMobileOpen(true)} aria-label={t('menu')}><Menu size={20} /></button></header>
    {mobileOpen && <button className="nav-scrim" onClick={() => setMobileOpen(false)} aria-label={t('close')} />}
    <aside className={`sidebar ${mobileOpen ? 'sidebar-open' : ''}`} role={mobileOpen ? 'dialog' : undefined} aria-modal={mobileOpen || undefined} aria-label={mobileOpen ? t('menu') : undefined} onKeyDown={trapNavigationFocus}>
      <div className="sidebar-top"><Brand name={branding.branding_name}/><button ref={closeButton} className="icon-button sidebar-close" onClick={() => setMobileOpen(false)} aria-label={t('close')}><X size={18} /></button></div>
      {projects.length > 1 && <label className="project-switcher"><span>{t('project')}</span><select value={project.id} onChange={(event) => setProjectId(event.target.value)}>{projects.filter((item)=>item.enabled).map((item)=><option key={item.id} value={item.id}>{item.name}</option>)}</select></label>}
      <nav aria-label={t('primaryNavigation')}>{groups.map((group)=><section className="nav-group" key={group.label}><h2>{group.label}</h2>{group.links.map(({ to, label, icon: Icon, end }) => <NavLink onClick={() => setMobileOpen(false)} className={({ isActive }) => isActive ? 'nav-link nav-link-active' : 'nav-link'} key={to} to={to} end={end}><Icon size={18} strokeWidth={1.8} /><span>{label}</span></NavLink>)}</section>)}</nav>
      <div className="account-block"><div className="account-identity"><span className="account-avatar" aria-hidden="true">{user.email.slice(0, 2)}</span><span title={user.email}>{user.email}</span></div><button className="signout" onClick={signOut}><LogOut size={16} aria-hidden="true" />{t('signOut')}</button></div>
    </aside>
    <main id="main-content" className="content" tabIndex={-1} inert={mobileOpen || undefined} aria-hidden={mobileOpen || undefined}>{!branding.onboarding_complete&&<aside className="onboarding-banner"><div><strong>{t('onboardingTitle')}</strong><span>{t('onboardingHint')}</span></div><NavLink className="button button-quiet" to="/channels">{t('configureChannel')}</NavLink></aside>}<Outlet /></main>
  </div>
}

function Brand({ compact = false,name }: { compact?: boolean;name:string }) {
  const { t } = useTranslation()
  return <div className={`brand ${compact ? 'brand-compact' : ''}`}><img src="/logo.webp" alt="" /><div><strong>{name||t('product')}</strong><span>{t('productSubtitle')}</span></div></div>
}
