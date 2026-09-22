import { useQuery } from '@tanstack/react-query'
import { useMemo } from 'react'
import { api, type Paged } from './api'
import { UNMEASURED } from './observability'
import { projectOperationPath, useProject } from './project'

/** Entity facets supported by `/analytics`; time is represented by its window. */
export const ANALYTICS_DIMENSIONS = ['provider', 'model', 'api_key', 'user', 'project'] as const
export type AnalyticsDimension = typeof ANALYTICS_DIMENSIONS[number]
export const ANALYTICS_DIMENSION_KEYS: Record<AnalyticsDimension, string> = {
  provider: 'provider',
  model: 'model',
  api_key: 'apiKey',
  user: 'user',
  project: 'project',
}

const DIMENSION_SOURCES: Record<AnalyticsDimension, { path: (projectId: string) => string; label: (row: Record<string, unknown>) => string } | null> = {
  provider: { path: (projectId) => `${projectOperationPath(projectId, 'channels')}?limit=500`, label: (row) => String(row.name ?? '') },
  model: { path: (projectId) => `${projectOperationPath(projectId, 'models')}?limit=500`, label: (row) => String(row.public_name ?? '') },
  api_key: { path: (projectId) => `/api/admin/v1/projects/${encodeURIComponent(projectId)}/api-keys`, label: (row) => String(row.name ?? '') },
  user: { path: () => '/api/admin/v1/users', label: (row) => String(row.display_name || row.email || '') },
  project: null,
}

/**
 * Resolves the opaque ids emitted by analytics into the names operators use in
 * the rest of the console. A missing or unreadable catalog degrades to the real
 * id instead of hiding an analytics row.
 */
export function useAnalyticsDimensionNames(projectId: string, dimension: AnalyticsDimension) {
  const { projects } = useProject()
  const source = DIMENSION_SOURCES[dimension]
  const query = useQuery({
    queryKey: ['dimension-names', projectId, dimension],
    queryFn: () => api<Array<Record<string, unknown>> | Paged<Record<string, unknown>>>(source!.path(projectId)),
    enabled: Boolean(source),
    retry: false,
  })
  const labels = useMemo(() => {
    const payload = query.data
    const rows = Array.isArray(payload) ? payload : payload?.data ?? []
    const map = new Map<string, string>()
    for (const row of rows) {
      const id = String(row.id ?? '')
      const label = source ? source.label(row) : ''
      if (id && label) map.set(id, label)
    }
    return map
  }, [query.data, source])
  const resolve = (value: string | null) => {
    if (!value) return { text: UNMEASURED, opaque: true }
    if (dimension === 'project') {
      const named = projects.find((item) => item.id === value)?.name
      return { text: named ?? value, opaque: !named }
    }
    const named = labels.get(value)
    return { text: named ?? value, opaque: !named }
  }

  return { query, resolve }
}
