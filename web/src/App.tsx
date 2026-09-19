import { useQuery } from '@tanstack/react-query'
import { Navigate, Route, Routes } from 'react-router'
import { api, type Bootstrap } from './api'
import { Login, Setup } from './Auth'
import Shell from './Shell'
import { Keys, Models, Overview, Providers, Requests, SettingsPage } from './Pages'
import { SkeletonRows } from './components'

export default function App() {
  const bootstrap = useQuery({ queryKey: ['bootstrap'], queryFn: () => api<Bootstrap>('/api/v1/bootstrap'), retry: false })
  if (bootstrap.isLoading) return <main className="boot-screen"><img src="/logo.webp" alt=""/><SkeletonRows count={2}/></main>
  if (bootstrap.isError) return <main className="boot-screen"><h1>Pangolin / 鲮鲤</h1><p>{bootstrap.error.message}</p></main>
  const state = bootstrap.data
  if (!state) return null
  if (!state.initialized) return <Routes><Route path="*" element={<Setup/>}/></Routes>
  if (!state.authenticated) return <Routes><Route path="/login" element={<Login/>}/><Route path="*" element={<Navigate to="/login" replace/>}/></Routes>
  return <Routes><Route element={<Shell/>}><Route index element={<Overview/>}/><Route path="providers" element={<Providers/>}/><Route path="models" element={<Models/>}/><Route path="keys" element={<Keys/>}/><Route path="requests" element={<Requests/>}/><Route path="settings" element={<SettingsPage/>}/></Route><Route path="*" element={<Navigate to="/" replace/>}/></Routes>
}
