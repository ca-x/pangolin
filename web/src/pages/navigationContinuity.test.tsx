import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { RequestDetailPage, TraceDetailPage } from './OperationsPage'

const response = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'a', enabled: true, is_default: true }

function renderDetail(path: '/operations/traces/t1' | '/operations/requests/r1', from?: string) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const url = String(input)
    if (url.endsWith('/projects')) return response([project])
    if (url.includes('/permissions')) return response(['*'])
    return response({ error: 'unavailable' }, 503)
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[{ pathname: path, state: from ? { from } : null }]}><ProjectProvider><Routes><Route path="/operations/traces/:id" element={<TraceDetailPage />} /><Route path="/operations/requests/:id" element={<RequestDetailPage />} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('detail return navigation', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('gives a directly opened trace a shareable return address', async () => {
    renderDetail('/operations/traces/t1')
    expect(await screen.findByRole('link', { name: /Back to traces/ })).toHaveAttribute('href', '/operations?tab=traces')
  })

  it('preserves request filters on the return link', async () => {
    renderDetail('/operations/requests/r1', '/operations?status=500&window=24h')
    expect(await screen.findByRole('link', { name: /Back to requests/ })).toHaveAttribute('href', '/operations?status=500&window=24h')
  })
})
