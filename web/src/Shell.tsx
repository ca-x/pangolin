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

export default function Shell({ user, branding }: { user: User; branding: Branding }) {
  return <ProjectProvider><ShellContent user={user} branding={branding} /></ProjectProvider>
}

function ShellContent({ user, branding }: { user: User; branding: Branding }) {
  const { t } = useTranslation()
  const route = useLocation()
  const isOverview = route.pathname === '/'
  const { project, projects, permissions, setProjectId } = useProject()
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
  const can = (permission: string) => permissions.has('*') || permissions.has(permission)
  // The guidance is dismissed per instance and remembered in this browser, so
  // hiding it is not the same decision as declaring onboarding finished for the
  // whole instance. It stays restorable from the appearance settings.
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => isOnboardingDismissed(branding.instance_name))
  useEffect(() => {
    const sync = () => setOnboardingDismissed(isOnboardingDismissed(branding.instance_name))
    window.addEventListener(ONBOARDING_CHANGED_EVENT, sync)
    return () => window.removeEventListener(ONBOARDING_CHANGED_EVENT, sync)
  }, [branding.instance_name])
  const canAccess = ['project:manage', 'api_key:manage', 'role:manage', 'user:manage', 'oidc:manage', 'project:read'].some(can)
  const groups: Array<{ label: string; links: NavLinkSpec[] }> = [
    { label: t('workspace'), links: [
      { to: '/', label: t('overview'), icon: Gauge },
      ...(can('project:manage') ? [{ to: '/channels', label: t('channels'), icon: Unplug }] : []),
      ...((can('project:manage') || can('catalog:manage')) ? [{ to: '/models', label: t('models'), icon: Boxes }] : []),
      ...(can('project:manage') ? [{ to: '/prompts', label: t('prompts'), icon: MessageSquareText }] : []),
      { to: '/playground', label: t('playground'), icon: FlaskConical },
    ] },
    { label: t('observe'), links: [
      { to: '/operations', label: t('operations'), icon: Activity, children: [
        { to: '/operations?tab=requests', label: t('requests'), icon: Activity },
        { to: '/operations?tab=traces', label: t('traces'), icon: ListTree },
        { to: '/operations?tab=threads', label: t('threads'), icon: MessageSquareText },
        { to: '/operations?tab=usage', label: t('usage'), icon: FileText },
      ] },
      { to: '/analytics', label: t('analytics'), icon: BarChart3 },
    ] },
    ...((canAccess || can('catalog:manage') || can('project:manage')) ? [{ label: t('administration'), links: [
      ...(canAccess ? [{ to: '/access', label: t('access'), icon: Shield, children: [
        ...(can('api_key:manage') || can('project:manage') ? [{ to: '/access?tab=keys', label: t('keys'), icon: KeyRound }, { to: '/access?tab=profiles', label: t('profiles'), icon: Layers3 }] : []),
        ...(can('project:manage') ? [{ to: '/access?tab=projects', label: t('projects'), icon: Boxes }, { to: '/access?tab=invitations', label: t('invitations'), icon: Mail }] : []),
        ...(can('user:manage') ? [{ to: '/access?tab=users', label: t('users'), icon: Users }] : []),
        ...(can('role:manage') || can('project:manage') ? [{ to: '/access?tab=roles', label: t('roles'), icon: Shield }] : []),
        ...(can('project:read') || can('project:manage') ? [{ to: '/access?tab=members', label: t('members'), icon: Users }] : []),
      ] }] : []),
      ...((can('catalog:manage') || can('project:manage')) ? [{ to: '/system', label: t('systemSettings'), icon: Settings, children: [
        { to: '/system?tab=appearance', label: t('appearance'), icon: SunMoon },
        ...(can('project:manage') ? [{ to: '/system?tab=storage', label: t('storage'), icon: Database }] : []),
      ] }] : []),
    ] }] : []),
  ]

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
          <Burger ref={menuTrigger} className="pm-mobile-menu-button" opened={mobileOpen} onClick={toggleMobile} aria-label={t('menu')} aria-expanded={mobileOpen} size="sm" />
        </Group>
      </AppShell.Header>

      {/* Sidebar (desktop: floating panel; mobile: Drawer via AppShell) */}
      <FocusTrap active={Boolean(mobile && mobileOpen)}><AppShell.Navbar
        p="md"
        className="pm-shell-navbar"
        data-open={mobileOpen ? 'true' : 'false'}
        role={mobile && mobileOpen ? 'dialog' : undefined}
        aria-modal={mobile && mobileOpen ? true : undefined}
        aria-label={mobile && mobileOpen ? t('primaryNavigation') : undefined}
      >
        <Group justify="flex-end" hiddenFrom="md" mb="xs"><ActionIcon className="pm-drawer-close" variant="subtle" color="gray" aria-label={t('close')} data-autofocus onClick={closeMobile}><X size={18} /></ActionIcon></Group>
        {/* The mobile header already carries the brand. */}
        <Group justify="space-between" mb="md" mt={2} mx={6} visibleFrom="md">
          <Brand name={branding.branding_name} />
        </Group>

        {/* Navigation */}
        <AppShell.Section grow component={Stack} gap="lg" className="pm-shell-navigation">
          <nav aria-label={t('primaryNavigation')}>
            {groups.map((group) => (
              <Stack key={group.label} gap={2} mb="md">
                <Text size="xs" fw={600} c="dimmed" px="xs" mb={4} style={{ letterSpacing: 0 }}>
                  {group.label}
                </Text>
                {group.links.map((item) => item.children?.length
                  ? <NavBranch key={item.to} item={item} onNavigate={closeMobile} />
                  : <NavItem key={item.to} {...item} onClick={closeMobile} />)}
              </Stack>
            ))}
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
            {projects.filter((item) => item.enabled).length > 1 ? (
              <Select aria-label={t('project')} value={project.id} onChange={(value) => value && setProjectId(value)} data={projects.filter((item) => item.enabled).map((item) => ({ value: item.id, label: item.name }))} size="xs" w={{ base: 160, sm: 220 }} />
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
        {/* Onboarding competes with the page's own primary action on every
            screen, so it only takes the full width on the overview. Elsewhere it
            drops its title and demotes its action to a text button, which keeps
            one solid primary per resource page. */}
        {can('*') && !branding.onboarding_complete && !onboardingDismissed && (
          <Alert variant="light" color="pangolin" mb="lg" radius="md" className="pm-onboarding" title={isOverview ? t('onboardingTitle') : undefined}>
            <Group justify="space-between" align="center" gap="md" wrap="nowrap" className="pm-onboarding-row">
              <Text size="sm">{t(isOverview ? 'onboardingHint' : 'onboardingProgressHint')}</Text>
              <Group gap="xs" wrap="nowrap">
                {isOverview ? (
                  <Button component={RouterLink} to="/channels" variant="light" size="sm">
                    {t('channels')}
                  </Button>
                ) : (
                  <Button component={RouterLink} to="/channels" variant="subtle" size="compact-sm">
                    {t('channels')}
                  </Button>
                )}
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
  const belongsHere = location.pathname === item.to || location.pathname.startsWith(item.to + '/')
  const [manual, setManual] = useState<boolean | null>(null)
  useEffect(() => setManual(null), [location.pathname])
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
  const [path, query] = to.split('?')
  const targetTab = new URLSearchParams(query).get('tab')
  const currentTab = new URLSearchParams(location.search).get('tab')
  const atPath = location.pathname === path
  const atDetail = path !== '/' && location.pathname.startsWith(path + '/')
  const active = (atPath || atDetail) && (targetTab ? atPath && targetTab === currentTab : !currentTab)

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
