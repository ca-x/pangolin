import { Alert, Center, Code, Stack, Title } from '@mantine/core'
import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense, useEffect } from 'react'
import { Navigate, Route, Routes, useLocation } from 'react-router'
import { useTranslation } from 'react-i18next'
import { api, type Bootstrap } from './api'
import { InvitationAccept, Login, Setup } from './Auth'
import Shell from './Shell'
import { SkeletonRows } from './components'
const AccessPage = lazy(() => import('./pages/AccessPage'))
const AccountPage = lazy(() => import('./pages/AccountPage'))
const AnalyticsPage = lazy(() => import('./pages/AnalyticsPage'))
const ChannelsPage = lazy(() => import('./pages/ChannelsPage'))
const ModelsPage = lazy(() => import('./pages/ModelsPage'))
const OperationsPage = lazy(() => import('./pages/OperationsPage'))
const RequestDetailPage = lazy(() => import('./pages/OperationsPage').then(module => ({ default: module.RequestDetailPage })))
const TraceDetailPage = lazy(() => import('./pages/OperationsPage').then(module => ({ default: module.TraceDetailPage })))
const OverviewPage = lazy(() => import('./pages/OverviewPage'))
const PlaygroundPage = lazy(() => import('./pages/PlaygroundPage'))
const PromptsPage = lazy(() => import('./pages/PromptsPage'))
const SystemPage = lazy(() => import('./pages/SystemPage'))

/** First path segment → the i18n key naming that screen for the tab title. */
const ROUTE_TITLES: Record<string, string> = {
  '': 'overview',
  channels: 'channels',
  models: 'models',
  access: 'access',
  account: 'account',
  prompts: 'prompts',
  operations: 'operations',
  analytics: 'analytics',
  playground: 'playground',
  system: 'systemSettings',
}
const ROUTE_TABS: Record<string, readonly string[]> = {
  channels: ['channels', 'credentials', 'channelPolicies', 'probes', 'quotas', 'presets'],
  models: ['models', 'routing', 'groups', 'prices', 'catalog', 'subscriptions'],
  access: ['keys', 'profiles', 'projects', 'users', 'members', 'roles', 'invitations', 'oidc', 'identities'],
  operations: ['requests', 'executions', 'threads', 'traces', 'usage', 'costItems', 'audit'],
  prompts: ['prompts', 'protection', 'overrides'],
  system: ['appearance', 'orchestrationSettings', 'storage', 'backups', 'webhooks', 'jobs', 'retention', 'modelSettings', 'requestLogging', 'proxyPresets', 'diagnostics', 'about'],
}

export default function App() {
  const { i18n } = useTranslation()
  const location = useLocation()
  const bootstrap = useQuery({ queryKey: ['bootstrap'], queryFn: () => api<Bootstrap>('/api/v1/bootstrap'), retry: false })
  useEffect(() => {
    const branding = bootstrap.data?.branding
    if (!branding) return
    // The browser tab is a navigation surface: name the screen the operator is
    // on instead of showing the instance label on every one of them.
    const first = location.pathname.split('/').filter(Boolean)[0] || ''
    const requestedTab = new URLSearchParams(location.search).get('tab')
    const section = requestedTab && ROUTE_TABS[first]?.includes(requestedTab) ? requestedTab : ROUTE_TITLES[first]
    document.title = section ? `${i18n.t(section)} · ${branding.branding_name}` : branding.branding_name
    let link = document.querySelector<HTMLLinkElement>("link[rel='icon']")
    if (!link) { link = document.createElement('link'); link.rel = 'icon'; document.head.append(link) }
    link.href = branding.favicon_url
  }, [bootstrap.data?.branding, location.pathname, location.search, i18n.language])

  if (bootstrap.isLoading) return (
    <Center h="100vh">
      <Stack align="center" gap="lg">
        <img src="/logo.webp" alt="" width={84} height={84} style={{ objectFit: 'contain' }} />
        <SkeletonRows count={2} />
      </Stack>
    </Center>
  )

  if (bootstrap.isError) return (
    <Center h="100vh" p="md">
      <Stack align="center" gap="md" style={{ maxWidth: 420 }}>
        <Title order={1}>Pangolin / 鲮鲤</Title>
        <Alert variant="light" color="red" style={{ width: '100%' }}>
          <Code block>{bootstrap.error.message}</Code>
        </Alert>
      </Stack>
    </Center>
  )

  const state = bootstrap.data
  if (!state) return null
  if (!state.initialized) return <Routes><Route path="/setup" element={<Setup />} /><Route path="*" element={<Navigate to="/setup" replace />} /></Routes>
  if (location.pathname === '/invite') return <Routes><Route path="/invite" element={<InvitationAccept authenticated={state.authenticated} />} /></Routes>
  if (!state.authenticated) return <Routes><Route path="/login" element={<Login />} /><Route path="*" element={<Navigate to="/login" replace />} /></Routes>
  const branding = state.branding || { instance_name: 'Pangolin', branding_name: 'Pangolin / 鲮鲤', favicon_url: '/logo.webp', onboarding_complete: true }
  return (
    <Suspense fallback={<Center h="100vh"><SkeletonRows /></Center>}>
      <Routes>
        <Route path="/login" element={<Navigate to="/" replace />} />
        <Route element={<Shell user={state.user!} branding={branding} />}>
          <Route index element={<OverviewPage />} />
          <Route path="channels" element={<ChannelsPage />} />
          <Route path="models" element={<ModelsPage />} />
          <Route path="access" element={<AccessPage />} />
          <Route path="account" element={<AccountPage />} />
          <Route path="prompts" element={<PromptsPage />} />
          <Route path="operations" element={<OperationsPage />} />
          <Route path="analytics" element={<AnalyticsPage />} />
          <Route path="operations/requests/:id" element={<RequestDetailPage />} />
          <Route path="operations/traces/:id" element={<TraceDetailPage />} />
          <Route path="playground" element={<PlaygroundPage />} />
          <Route path="system" element={<SystemPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Suspense>
  )
}
