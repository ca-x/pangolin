import { AppShell, ActionIcon, Avatar, Burger, Button, FocusTrap, Group, Menu, NavLink as MantineNavLink, Select, Stack, Text, Alert, Tooltip, UnstyledButton } from '@mantine/core'
import { useDisclosure, useMediaQuery } from '@mantine/hooks'
import { Activity, BarChart3, Boxes, ChevronDown, Database, FileText, FlaskConical, Gauge, KeyRound, Languages, Layers3, ListTree, LogOut, Mail, MessageSquareText, Settings, Shield, SunMoon, Unplug, UserRound, Users, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useEffect, useRef, useState } from 'react'
import { Link as RouterLink, Outlet, useLocation, useNavigate } from 'react-router'
import { api } from './api'
import type { Branding, User } from './api'
import i18n from './i18n'
import { ONBOARDING_CHANGED_EVENT, dismissOnboarding, isOnboardingDismissed } from './onboarding'
import { ProjectProvider, useProject } from './project'
import { useTheme } from './theme'

type NavLinkSpec = { to: string; label: string; icon: typeof Gauge; children?: NavLinkSpec[] }
const DEFAULT_TAB_FOR_PATH: Record<string, string> = { '/channels': 'channels', '/models': 'models', '/access': 'keys', '/prompts': 'prompts', '/operations': 'requests', '/system': 'appearance' }

function navLinkMatches(to: string, pathname: string, search: string) {
  const [path, query] = to.split('?')
  const targetTab = new URLSearchParams(query).get('tab')
  const currentTab = new URLSearchParams(search).get('tab')
  const effectiveTab = currentTab ?? DEFAULT_TAB_FOR_PATH[path] ?? null
  const atPath = pathname === path
  const atDetail = path !== '/' && pathname.startsWith(path + '/')
  const detailTab = path === '/operations' && atDetail ? pathname.split('/')[2] : null
  if (path === '/system') return atPath
  return targetTab
    ? (atPath && targetTab === effectiveTab) || (atDetail && targetTab === detailTab)
    : (atDetail && DEFAULT_TAB_FOR_PATH[path] === detailTab) || (atPath && (!currentTab || currentTab === DEFAULT_TAB_FOR_PATH[path]))
}

export default function Shell({ user, branding }: { user: User; branding: Branding }) {
  return <ProjectProvider><ShellContent user={user} branding={branding} /></ProjectProvider>
}

function ShellContent({ user, branding }: { user: User; branding: Branding }) {
  const { t } = useTranslation()
  const route = useLocation()
  const isOverview = route.pathname === '/'
  const { project, projects, permissions, permissionsStatus, retryPermissions, setProjectId } = useProject()
  const enabledProjects = projects.filter((item) => item.enabled)
  const { mode, setMode } = useTheme()
  const navigate = useNavigate()
  const [mobileOpen, { toggle: toggleMobile, close: closeMobile }] = useDisclosure(false)
  const menuTrigger = useRef<HTMLButtonElement>(null)
  const wasMobileOpen = useRef(false)
  // Below the md breakpoint the navbar is an off-canvas drawer, and an off-canvas
  // drawer has to behave like one: dismissible, and modal for assistive tech.
  const mobile = useMediaQuery('(max-width: 61.99em)', true, { getInitialValueInEffect: false })
  useEffect(() => {
    if (!mobile || !mobileOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeMobile()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [mobile, mobileOpen, closeMobile])
  useEffect(() => {
    if (mobile && wasMobileOpen.current && !mobileOpen) requestAnimationFrame(() => menuTrigger.current?.focus())
    wasMobileOpen.current = mobileOpen
  }, [mobile, mobileOpen])
  const can = (permission: string) => permissionsStatus === 'ready' && (permissions.has('*') || permissions.has(permission))
  // The guidance is dismissed per instance and remembered in this browser, so
  // hiding it is not the same decision as declaring onboarding finished for the
  // whole instance. It stays restorable from the appearance settings.
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => isOnboardingDismissed(branding.instance_name))
  useEffect(() => {
    const sync = () => setOnboardingDismissed(isOnboardingDismissed(branding.instance_name))
    window.addEventListener(ONBOARDING_CHANGED_EVENT, sync)
    return () => window.removeEventListener(ONBOARDING_CHANGED_EVENT, sync)
  }, [branding.instance_name])
  const canManageProject = can('project:manage')
  const canReadProject = can('project:read') || canManageProject
  const canManageKeys = can('api_key:manage') || canManageProject
  const canManageRoles = can('role:manage') || canManageProject
  const canManageCatalog = can('catalog:manage')
  const adminLinks: NavLinkSpec[] = [
    ...(canManageProject ? [{ to: '/access?tab=projects', label: t('projects'), icon: Boxes }, { to: '/access?tab=invitations', label: t('invitations'), icon: Mail }] : []),
    ...(can('user:manage') ? [{ to: '/access?tab=users', label: t('users'), icon: Users }] : []),
    ...(canManageRoles ? [{ to: '/access?tab=roles', label: t('roles'), icon: Shield }] : []),
    ...(can('oidc:manage') ? [{ to: '/access?tab=oidc', label: 'OIDC', icon: Shield }, { to: '/access?tab=identities', label: t('identities'), icon: Users }] : []),
    ...((canManageProject || canManageCatalog) ? [{ to: '/system', label: t('systemSettings'), icon: Settings }] : []),
  ]
  const groups: Array<{ id: string; label: string; links: NavLinkSpec[] }> = [
    { id: 'workspace', label: t('workspace'), links: [
      { to: '/', label: t('overview'), icon: Gauge },
      ...(canManageProject ? [{ to: '/channels', label: t('channels'), icon: Unplug, children: [
        { to: '/channels?tab=credentials', label: t('credentials'), icon: KeyRound },
        { to: '/channels?tab=channelPolicies', label: t('channelPolicies'), icon: Shield },
        { to: '/channels?tab=probes', label: t('probes'), icon: Activity },
        { to: '/channels?tab=quotas', label: t('quotas'), icon: BarChart3 },
        { to: '/channels?tab=presets', label: t('presets'), icon: Database },
      ] }] : []),
      ...((canManageProject || canManageCatalog) ? [{ to: canManageProject ? '/models' : '/models?tab=catalog', label: canManageProject ? t('models') : t('catalog'), icon: Boxes, children: [
        ...(canManageProject ? [
          { to: '/models?tab=routing', label: t('routing'), icon: ListTree },
          { to: '/models?tab=groups', label: t('serviceGroups'), icon: Layers3 },
          { to: '/models?tab=prices', label: t('prices'), icon: FileText },
          { to: '/models?tab=catalog', label: t('catalog'), icon: Database },
        ] : []),
        ...(canManageCatalog ? [{ to: '/models?tab=subscriptions', label: t('subscriptions'), icon: Database }] : []),
      ] }] : []),
      ...(canManageKeys ? [{ to: '/access?tab=keys', label: t('keys'), icon: KeyRound }, { to: '/access?tab=profiles', label: t('profiles'), icon: Layers3 }] : []),
      ...(canReadProject ? [{ to: '/access?tab=members', label: t('members'), icon: Users }] : []),
      ...(canManageProject ? [{ to: '/prompts', label: t('prompts'), icon: MessageSquareText, children: [
        { to: '/prompts?tab=protection', label: t('protection'), icon: Shield },
        { to: '/prompts?tab=overrides', label: t('overrides'), icon: Settings },
      ] }] : []),
      { to: '/playground', label: t('playground'), icon: FlaskConical },
    ] },
    ...(adminLinks.length ? [{ id: 'administration', label: t('administration'), links: adminLinks }] : []),
    ...(canReadProject ? [{ id: 'observe', label: t('observe'), links: [
      { to: '/operations', label: t('requests'), icon: Activity },
      { to: '/operations?tab=traces', label: t('traces'), icon: ListTree },
      { to: '/operations?tab=threads', label: t('threads'), icon: MessageSquareText },
      { to: '/operations?tab=usage', label: t('usage'), icon: FileText },
      { to: '/analytics', label: t('analytics'), icon: BarChart3 },
    ] }] : []),
  ]
  const activeGroup = groups.find((group) => group.links.some((item) => navLinkMatches(item.to, route.pathname, route.search) || item.children?.some((child) => navLinkMatches(child.to, route.pathname, route.search))))?.id
  const [manualGroup, setManualGroup] = useState<string | null>(null)
  useEffect(() => setManualGroup(null), [route.pathname, route.search])
  const expandedGroup = manualGroup ?? activeGroup ?? groups[0]?.id

  const signOut = async () => { await api('/api/v1/auth/logout', { method: 'POST' }); navigate('/login'); location.reload() }

  return (
    <AppShell
      header={{ height: { base: 60, md: 0 } }}
      navbar={{
        width: 260,
        breakpoint: 'md',
        collapsed: { mobile: !mobileOpen, desktop: false },
      }}
      padding="md"
    >
      {/* Skip link */}
      <a href="#main-content" style={{
        position: 'fixed', left: 16, top: 12, zIndex: 150,
        padding: '10px 14px', borderRadius: 'var(--mantine-radius-md)',
        background: 'var(--mantine-color-body)', color: 'var(--mantine-color-text)',
        boxShadow: 'var(--mantine-shadow-lg)', transform: 'translateY(-200%)',
        transition: 'transform 160ms ease',
      }} onFocus={(e) => { (e.currentTarget as HTMLElement).style.transform = 'translateY(0)' }}
        onBlur={(e) => { (e.currentTarget as HTMLElement).style.transform = 'translateY(-200%)' }}>
        {t('skipToContent')}
      </a>

      {/* Mobile header */}
      <AppShell.Header hiddenFrom="md" px="sm">
        <Group justify="space-between" h="100%">
          <Brand compact name={branding.branding_name} />
          <Burger ref={menuTrigger} className="pm-mobile-menu-button" opened={mobileOpen} onClick={toggleMobile} aria-label={t('menu')} aria-expanded={mobileOpen} size="sm" style={{ visibility: mobileOpen ? 'hidden' : 'visible' }} />
        </Group>
      </AppShell.Header>

      {/* Sidebar (desktop: floating panel; mobile: Drawer via AppShell) */}
      <FocusTrap active={Boolean(mobile && mobileOpen)}><AppShell.Navbar
        component="div"
        p="md"
        className="pm-shell-navbar"
        data-open={mobileOpen ? 'true' : 'false'}
        role={mobile && mobileOpen ? 'dialog' : 'complementary'}
        aria-modal={mobile && mobileOpen ? true : undefined}
        aria-label={mobile && mobileOpen ? t('primaryNavigation') : t('navigationAndAccount')}
      >
        <Group justify="flex-end" hiddenFrom="md" mb="xs"><ActionIcon className="pm-drawer-close" variant="subtle" color="gray" aria-label={t('close')} data-autofocus onClick={closeMobile}><X size={18} /></ActionIcon></Group>
        {/* The mobile header already carries the brand. */}
        <Stack gap="md" mb="md" mt={2} mx={6} visibleFrom="md">
          <Brand name={branding.branding_name} />
          <div className="pm-nav-project" role="group" aria-label={t('project')}>
            {enabledProjects.length > 1 ? (
              <Select aria-label={t('project')} value={project.id} onChange={(value) => value && setProjectId(value)} data={enabledProjects.map((item) => ({ value: item.id, label: item.name }))} size="sm" />
            ) : <Text fw={600} size="sm" truncate="end" title={project.name}>{project.name}</Text>}
          </div>
        </Stack>

        {/* Navigation */}
        <AppShell.Section grow component={Stack} gap="lg" className="pm-shell-navigation">
          <nav aria-label={t('primaryNavigation')}>
            {groups.map((group) => <section key={group.id} className="pm-nav-section" aria-label={group.label}>
              <UnstyledButton className="pm-nav-section-toggle" data-current={activeGroup === group.id ? 'true' : undefined} aria-expanded={expandedGroup === group.id} aria-controls={`pm-nav-${group.id}`} onClick={() => setManualGroup(expandedGroup === group.id ? 'none' : group.id)}>
                <span>{group.label}</span><ChevronDown size={15} aria-hidden="true" className={expandedGroup === group.id ? 'is-open' : undefined} />
              </UnstyledButton>
              <div id={`pm-nav-${group.id}`} className="pm-nav-section-links" hidden={expandedGroup !== group.id}>
                {group.links.map((item) => item.children?.length
                  ? <NavBranch key={item.to} item={item} onNavigate={closeMobile} />
                  : <NavItem key={item.to} {...item} onClick={closeMobile} />)}
              </div>
            </section>)}
          </nav>
        </AppShell.Section>

        {/* Account area */}
        <AppShell.Section mt="auto" pt="sm" style={{ borderTop: '1px solid var(--mantine-color-default-border)' }}>
          <Menu shadow="md" width={200}>
            <Menu.Target>
              <UnstyledButton px="xs" py={4} w="100%" style={{ borderRadius: 'var(--mantine-radius-md)' }}>
                <Group gap="sm" wrap="nowrap">
                  <Avatar color="pangolin" radius="md" size={28}>{user.email.slice(0, 2).toUpperCase()}</Avatar>
                  <Text size="xs" c="dimmed" truncate="end" style={{ maxWidth: 160 }} title={user.email}>{user.email}</Text>
                </Group>
              </UnstyledButton>
            </Menu.Target>
            <Menu.Dropdown>
              <Menu.Item component={RouterLink} to="/account" leftSection={<UserRound size={16} />}>
                {t('account')}
              </Menu.Item>
              <Menu.Item leftSection={<LogOut size={16} />} onClick={signOut}>
                {t('signOut')}
              </Menu.Item>
            </Menu.Dropdown>
          </Menu>
        </AppShell.Section>
      </AppShell.Navbar></FocusTrap>

      {/* Main content */}
      {mobile && mobileOpen && (
        <button type="button" className="nav-scrim" aria-label={t('close')} onClick={closeMobile} />
      )}

      <AppShell.Main id="main-content" inert={mobile && mobileOpen ? true : undefined}>
        <div className="pm-workspace-bar">
          <div className="pm-workspace-context">
            <Text size="xs" c="dimmed">{t('project')}</Text>
            {enabledProjects.length > 1 ? (
              <Select aria-label={t('project')} value={project.id} onChange={(value) => value && setProjectId(value)} data={enabledProjects.map((item) => ({ value: item.id, label: item.name }))} size="xs" w={{ base: 160, sm: 220 }} />
            ) : <Text fw={600} size="sm" truncate="end">{project.name}</Text>}
          </div>
          <Group gap={4} wrap="nowrap">
            <Tooltip label={t('language')}><ActionIcon variant="subtle" color="gray" aria-label={t('language')} onClick={() => void i18n.changeLanguage(i18n.language === 'zh-CN' ? 'en' : 'zh-CN')}><Languages size={18} /></ActionIcon></Tooltip>
            <Menu shadow="md" width={160}>
              <Menu.Target><Tooltip label={t('colorMode')}><ActionIcon variant="subtle" color="gray" aria-label={t('colorMode')}><SunMoon size={18} /></ActionIcon></Tooltip></Menu.Target>
              <Menu.Dropdown>{(['system', 'light', 'dark'] as const).map((value) => <Menu.Item key={value} onClick={() => setMode(value)} fw={mode === value ? 650 : 400}>{t(value)}</Menu.Item>)}</Menu.Dropdown>
            </Menu>
            {(can('catalog:manage') || can('project:manage')) && <Tooltip label={t('systemSettings')}><ActionIcon component={RouterLink} to="/system" variant="subtle" color="gray" aria-label={t('settings')}><Settings size={18} /></ActionIcon></Tooltip>}
          </Group>
        </div>
        {permissionsStatus === 'error' && !['/access', '/models', '/system'].includes(route.pathname) && <Alert variant="light" color="red" mb="lg" title={t('networkError')}>
          <Group justify="space-between" gap="sm"><Text size="sm">{t('retryHint')}</Text><Button variant="outline" color="red" size="compact-sm" onClick={retryPermissions}>{t('retry')}</Button></Group>
        </Alert>}
        {isOverview && can('*') && !branding.onboarding_complete && !onboardingDismissed && (
          <Alert variant="light" color="pangolin" mb="lg" radius="md" className="pm-onboarding" title={t('onboardingTitle')}>
            <Group justify="space-between" align="center" gap="md" wrap="nowrap" className="pm-onboarding-row">
              <Text size="sm">{t('onboardingHint')}</Text>
              <Group gap="xs" wrap="nowrap">
                <Button component={RouterLink} to="/channels" variant="light" size="sm">{t('channels')}</Button>
                {/* Dismissing hides the guidance here only; the branding switch
                    still records that onboarding is finished instance-wide. */}
                <Tooltip label={t('dismissOnboarding')}>
                  <ActionIcon
                    variant="subtle"
                    color="gray"
                    aria-label={t('dismissOnboarding')}
                    onClick={() => { dismissOnboarding(branding.instance_name); setOnboardingDismissed(true) }}
                  >
                    <X size={17} />
                  </ActionIcon>
                </Tooltip>
              </Group>
            </Group>
          </Alert>
        )}
        <Outlet />
      </AppShell.Main>
    </AppShell>
  )
}

function NavBranch({ item, onNavigate }: { item: NavLinkSpec; onNavigate: () => void }) {
  const { t } = useTranslation()
  const location = useLocation()
  const path = item.to.split('?')[0]
  const belongsHere = location.pathname === path || location.pathname.startsWith(path + '/')
  const [manual, setManual] = useState<boolean | null>(null)
  useEffect(() => setManual(null), [location.pathname, location.search])
  const expanded = manual ?? belongsHere
  return <div className="pm-nav-branch">
    <Group gap={2} wrap="nowrap">
      <NavItem to={item.to} label={item.label} icon={item.icon} onClick={onNavigate} />
      <ActionIcon variant="subtle" color="gray" className="pm-nav-disclosure" aria-expanded={expanded} aria-label={t(expanded ? 'hideSubpages' : 'showSubpages', { name: item.label })} onClick={() => setManual(!expanded)}><ChevronDown size={16} className={expanded ? 'is-open' : undefined} /></ActionIcon>
    </Group>
    {expanded && <div className="pm-nav-sublist">{item.children?.map((child) => <NavItem key={child.to} {...child} nested onClick={onNavigate} />)}</div>}
  </div>
}

function NavItem({ to, label, icon: Icon, nested = false, onClick }: {
  to: string
  label: string
  icon: typeof Gauge
  nested?: boolean
  onClick: () => void
}) {
  const location = useLocation()
  const active = navLinkMatches(to, location.pathname, location.search)

  return (
    <MantineNavLink
      className={nested ? 'pm-nav-child' : undefined}
      component={RouterLink}
      to={to}
      label={label}
      leftSection={<Icon size={18} strokeWidth={1.8} />}
      active={active}
      onClick={onClick}
    />
  )
}

function Brand({ compact = false, name }: { compact?: boolean; name: string }) {
  const { t } = useTranslation()
  return (
    <Group gap="sm" wrap="nowrap" style={{ minWidth: 0 }}>
      <img src="/logo.webp" alt="" width={compact ? 32 : 38} height={compact ? 32 : 38}
        style={{ objectFit: 'contain', filter: 'drop-shadow(0 2px 3px rgb(45 25 18 / .12))' }} />
      <Stack gap={0} style={{ minWidth: 0 }}>
        <Text fw={620} size="md" lh="1.25" style={{ letterSpacing: '-.012em' }}>{name || t('product')}</Text>
        <Text size="xs" c="dimmed">{t('productSubtitle')}</Text>
      </Stack>
    </Group>
  )
}
