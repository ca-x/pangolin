import { useQuery } from '@tanstack/react-query'
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api, type Project } from './api'
import { SkeletonRows } from './components'
import { useTranslation } from 'react-i18next'

/**
 * Whether the active project's permission list is known. "The answer has not
 * arrived" and "the answer grants nothing" are different facts, and a console
 * that renders them the same way tells the operator they have no access while
 * the answer is still in flight.
 */
export type PermissionsStatus = 'loading' | 'ready' | 'error'

type ProjectState = {
  project: Project
  projects: Project[]
  permissions: Set<string>
  permissionsStatus: PermissionsStatus
  retryPermissions: () => void
  setProjectId: (id: string) => void
}
const ProjectContext = createContext<ProjectState | null>(null)

export function ProjectProvider({ children }: { children: ReactNode }) {
  const {t}=useTranslation()
  const query = useQuery({ queryKey: ['projects'], queryFn: () => api<Project[]>('/api/admin/v1/projects') })
  const [selected, setSelected] = useState(() => localStorage.getItem('pangolin-project') || '')
  const projects = query.data || []
  const project = projects.find((item) => item.id === selected && item.enabled) || projects.find((item) => item.is_default && item.enabled) || projects.find((item) => item.enabled)
  const permissions = useQuery({ queryKey:['project-permissions',project?.id], queryFn:()=>api<string[]>(`/api/admin/v1/projects/${project!.id}/permissions`), enabled:Boolean(project) })
  useEffect(() => { if (project) { setSelected(project.id); localStorage.setItem('pangolin-project', project.id) } }, [project?.id])
  // The query is keyed by the project, so `data` is this project's own answer or nothing
  // at all: a switch cannot make the previous project's permissions stand in for the new
  // one. The status says which of the three it is, because a consumer that reads "no answer
  // yet" — or "the answer could not be confirmed" — as "no permissions" states an
  // authorization fact it has not been told.
  //
  // An answer already cached for this project counts as answered, but a read that *failed*
  // is the error state whether or not an older answer is still in the cache: the console
  // cannot confirm the permissions it would be acting on, so it must stop offering the
  // controls that answer authorized and offer the retry instead. Continuing to render them
  // from the cache would be authorizing the UI with an answer it could not re-read.
  const permissionsStatus: PermissionsStatus = !project || permissions.isPending ? 'loading' : permissions.isError ? 'error' : 'ready'
  const refetchPermissions = permissions.refetch
  const retryPermissions = useCallback(() => { void refetchPermissions() }, [refetchPermissions])
  const value = useMemo(() => project ? { project, projects, permissions:new Set(permissions.data||[]), permissionsStatus, retryPermissions, setProjectId: setSelected } : null, [permissions.data, permissionsStatus, project, projects, retryPermissions])
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
