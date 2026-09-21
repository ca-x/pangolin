import { useQuery } from '@tanstack/react-query'
import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api, type Project } from './api'
import { SkeletonRows } from './components'
import { useTranslation } from 'react-i18next'

type ProjectState = { project: Project; projects: Project[]; permissions: Set<string>; setProjectId: (id: string) => void }
const ProjectContext = createContext<ProjectState | null>(null)

export function ProjectProvider({ children }: { children: ReactNode }) {
  const {t}=useTranslation()
  const query = useQuery({ queryKey: ['projects'], queryFn: () => api<Project[]>('/api/admin/v1/projects') })
  const [selected, setSelected] = useState(() => localStorage.getItem('pangolin-project') || '')
  const projects = query.data || []
  const project = projects.find((item) => item.id === selected && item.enabled) || projects.find((item) => item.is_default && item.enabled) || projects.find((item) => item.enabled)
  const permissions = useQuery({ queryKey:['project-permissions',project?.id], queryFn:()=>api<string[]>(`/api/admin/v1/projects/${project!.id}/permissions`), enabled:Boolean(project) })
  useEffect(() => { if (project) { setSelected(project.id); localStorage.setItem('pangolin-project', project.id) } }, [project?.id])
  const value = useMemo(() => project ? { project, projects, permissions:new Set(permissions.data||[]), setProjectId: setSelected } : null, [permissions.data, project, projects])
  if (query.isLoading) return <SkeletonRows count={3}/>
  if (query.isError) return <div className="query-error" role="alert"><div><strong>{t('networkError')}</strong><p>{t('retryHint')}</p></div><button className="button" onClick={() => void query.refetch()}>{t('retry')}</button></div>
  if (!value) return <div className="query-error" role="alert">{t('noAccessibleProject')}</div>
  return <ProjectContext.Provider value={value}>{children}</ProjectContext.Provider>
}

export function useProject() {
  const value = useContext(ProjectContext)
  if (!value) throw new Error('useProject must be used inside ProjectProvider')
  return value
}

export const projectOperationPath = (project: string, resource: string, id?: string) => `/api/admin/v1/projects/${encodeURIComponent(project)}/operations/${resource}${id ? `/${encodeURIComponent(id)}` : ''}`
