import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { Navigate, Route, Routes } from 'react-router'
import { api, type Bootstrap } from './api'
import { Login, Setup } from './Auth'
import Shell from './Shell'
import { SkeletonRows } from './components'
const AccessPage=lazy(()=>import('./pages/AccessPage'))
const ChannelsPage=lazy(()=>import('./pages/ChannelsPage'))
const ModelsPage=lazy(()=>import('./pages/ModelsPage'))
const OperationsPage=lazy(()=>import('./pages/OperationsPage'))
const RequestDetailPage=lazy(()=>import('./pages/OperationsPage').then(module=>({default:module.RequestDetailPage})))
const TraceDetailPage=lazy(()=>import('./pages/OperationsPage').then(module=>({default:module.TraceDetailPage})))
const OverviewPage=lazy(()=>import('./pages/OverviewPage'))
const PlaygroundPage=lazy(()=>import('./pages/PlaygroundPage'))
const PromptsPage=lazy(()=>import('./pages/PromptsPage'))
const SystemPage=lazy(()=>import('./pages/SystemPage'))

export default function App() {
  const bootstrap = useQuery({ queryKey: ['bootstrap'], queryFn: () => api<Bootstrap>('/api/v1/bootstrap'), retry: false })
  if (bootstrap.isLoading) return <main className="boot-screen"><img src="/logo.webp" alt=""/><SkeletonRows count={2}/></main>
  if (bootstrap.isError) return <main className="boot-screen"><h1>Pangolin / 鲮鲤</h1><p>{bootstrap.error.message}</p></main>
  const state = bootstrap.data
  if (!state) return null
  if (!state.initialized) return <Routes><Route path="/setup" element={<Setup/>}/><Route path="*" element={<Navigate to="/setup" replace/>}/></Routes>
  if (!state.authenticated) return <Routes><Route path="/login" element={<Login/>}/><Route path="*" element={<Navigate to="/login" replace/>}/></Routes>
  return <Suspense fallback={<main className="content"><SkeletonRows/></main>}><Routes><Route path="/login" element={<Navigate to="/" replace/>}/><Route element={<Shell user={state.user!}/>}><Route index element={<OverviewPage/>}/><Route path="channels" element={<ChannelsPage/>}/><Route path="models" element={<ModelsPage/>}/><Route path="access" element={<AccessPage/>}/><Route path="prompts" element={<PromptsPage/>}/><Route path="operations" element={<OperationsPage/>}/><Route path="operations/requests/:id" element={<RequestDetailPage/>}/><Route path="operations/traces/:id" element={<TraceDetailPage/>}/><Route path="playground" element={<PlaygroundPage/>}/><Route path="system" element={<SystemPage/>}/></Route><Route path="*" element={<Navigate to="/" replace/>}/></Routes></Suspense>
}
