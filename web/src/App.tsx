import { Alert, Center, Code, Stack, Title } from '@mantine/core'
import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense, useEffect } from 'react'
import { Navigate, Route, Routes, useLocation } from 'react-router'
import { api, type Bootstrap } from './api'
import { InvitationAccept, Login, Setup } from './Auth'
import Shell from './Shell'
import { SkeletonRows } from './components'
const AccessPage = lazy(() => import('./pages/AccessPage'))
const AccountPage = lazy(() => import('./pages/AccountPage'))
const ChannelsPage = lazy(() => import('./pages/ChannelsPage'))
const ModelsPage = lazy(() => import('./pages/ModelsPage'))
const OperationsPage = lazy(() => import('./pages/OperationsPage'))
const RequestDetailPage = lazy(() => import('./pages/OperationsPage').then(module => ({ default: module.RequestDetailPage })))
const TraceDetailPage = lazy(() => import('./pages/OperationsPage').then(module => ({ default: module.TraceDetailPage })))
const OverviewPage = lazy(() => import('./pages/OverviewPage'))
const PlaygroundPage = lazy(() => import('./pages/PlaygroundPage'))
const PromptsPage = lazy(() => import('./pages/PromptsPage'))
const SystemPage = lazy(() => import('./pages/SystemPage'))

export default function App() {
  const location = useLocation()
  const bootstrap = useQuery({ queryKey: ['bootstrap'], queryFn: () => api<Bootstrap>('/api/v1/bootstrap'), retry: false })
  useEffect(() => {
    const branding = bootstrap.data?.branding
    if (!branding) return
    document.title = branding.branding_name
    let link = document.querySelector<HTMLLinkElement>("link[rel='icon']")
    if (!link) { link = document.createElement('link'); link.rel = 'icon'; document.head.append(link) }
    link.href = branding.favicon_url
  }, [bootstrap.data?.branding])

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
