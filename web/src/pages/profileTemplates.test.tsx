import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ConfirmHost } from '../components'
import { ProjectProvider, useProject } from '../project'
import AccessPage from './AccessPage'

const reply = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true }
const profile = { id: 'profile-a', name: 'Mobile', rpm_limit: 12, tpm_limit: 5000, budget_micros: 9000, routing_policy: { version: 1 }, mappings: [], allowed_models: [] }
const template = { id: 'template-a', name: 'Mobile defaults', profile: { version: 1, rpm_limit: 12, tpm_limit: 5000, budget_micros: 9000, routing_policy: { version: 1 }, mappings: [], allowed_models: [] }, created_at: 1, updated_at: 1 }
const templateB = { ...template, id: 'template-b', name: 'Desktop defaults' }
const otherProject = { ...project, id: 'project-b', name: 'Project B', slug: 'project-b', is_default: false }
const ProjectSwitch = () => { const { setProjectId } = useProject(); return <button type="button" onClick={() => setProjectId('project-b')}>Switch project</button> }
const deferred = <T,>() => {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

describe('API-key profile templates', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('saves, previews and confirms an apply, and exposes import and export', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project])
      if (path.includes('/permissions')) return reply(['api_key:manage', 'project:read'])
      if (path.includes('/operations/key-profiles')) return reply({ data: [profile], total: 1 })
      if (path.endsWith('/profile-templates') && init?.method === 'POST') return reply({ id: 'created-template' })
      if (path.includes('/profile-templates?')) return reply({ data: [template], total: 1 })
      if (path.endsWith('/template-a/export')) return reply({ version: 1, name: template.name, profile: template.profile })
      if (path.endsWith('/template-a/apply')) return reply({ id: profile.id })
      if (path.endsWith('/profile-templates/import')) return reply({ id: 'imported-template' })
      return reply([])
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Key profiles' }))

    await userEvent.click((await screen.findAllByRole('button', { name: 'Save as template' }))[0])
    const saveDialog = await screen.findByRole('dialog')
    await userEvent.type(within(saveDialog).getByRole('textbox', { name: /Template name/ }), 'Client defaults')
    await userEvent.click(within(saveDialog).getByRole('button', { name: 'Save template' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.some(([path, init]) => String(path).endsWith('/profile-templates') && init?.method === 'POST' && String(init.body).includes('profile-a'))).toBe(true))

    await userEvent.click(await screen.findByRole('button', { name: 'Apply Mobile defaults' }))
    const applyDialog = await screen.findByRole('dialog')
    expect(within(applyDialog).getByText(/12 RPM/)).toBeInTheDocument()
    await userEvent.click(within(applyDialog).getByRole('button', { name: 'Review apply' }))
    const confirmation = await screen.findByRole('dialog')
    expect(within(confirmation).getByText(/Mobile defaults/)).toBeInTheDocument()
    await userEvent.click(within(confirmation).getByRole('button', { name: 'Apply template' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.some(([path, init]) => String(path).endsWith('/template-a/apply') && init?.method === 'POST')).toBe(true))

    await userEvent.click(screen.getByRole('button', { name: 'Export Mobile defaults' }))
    expect(await screen.findByDisplayValue(/"Mobile defaults"/)).toBeInTheDocument()
    const exportDialog = await screen.findByRole('dialog')
    await userEvent.click(within(exportDialog).getAllByRole('button', { name: 'Close' }).at(-1)!)
    await userEvent.click(screen.getByRole('button', { name: 'Import template' }))
    expect(await screen.findByLabelText('Template JSON')).toBeInTheDocument()
  })

  it('shows retry and a useful empty state when the template query recovers', async () => {
    let reads = 0
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project])
      if (path.includes('/permissions')) return reply(['api_key:manage'])
      if (path.includes('/operations/key-profiles')) return reply({ data: [profile], total: 1 })
      if (path.includes('/profile-templates?')) { reads += 1; return reads === 1 ? reply({ error: { message: 'unavailable' } }, 503) : reply({ data: [], total: 0 }) }
      return reply([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Key profiles' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Retry' }))
    expect(await screen.findByText('No profile templates in this project yet.')).toBeInTheDocument()
  })

  it('refuses to save a template after the selected project changes', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project, otherProject])
      if (path.includes('/permissions')) return reply(['api_key:manage'])
      if (path.includes('/operations/key-profiles')) return reply({ data: [profile], total: 1 })
      if (path.includes('/profile-templates?')) return reply({ data: [template], total: 1 })
      return reply([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ProjectSwitch /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Key profiles' }))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Save as template' }))[0])
    await userEvent.click(screen.getByRole('button', { name: 'Switch project' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('selected project changed')
    expect(screen.getByRole('button', { name: 'Save template' })).toBeDisabled()
  })

  it('does not show an export that finishes after the selected project changes', async () => {
    const exported = deferred<Response>()
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project, otherProject])
      if (path.includes('/permissions')) return reply(['api_key:manage'])
      if (path.includes('/operations/key-profiles')) return reply({ data: [profile], total: 1 })
      if (path.includes('/profile-templates?')) return reply({ data: [template], total: 1 })
      if (path.endsWith('/template-a/export')) return exported.promise
      return reply([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ProjectSwitch /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Key profiles' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Export Mobile defaults' }))
    await userEvent.click(screen.getByRole('button', { name: 'Switch project' }))
    await act(async () => { exported.resolve(await reply({ version: 1, name: 'Mobile defaults', profile: template.profile })) })
    expect(screen.queryByDisplayValue(/"Mobile defaults"/)).not.toBeInTheDocument()
  })

  it('keeps the newest export when responses finish out of order', async () => {
    const first = deferred<Response>()
    const second = deferred<Response>()
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project])
      if (path.includes('/permissions')) return reply(['api_key:manage'])
      if (path.includes('/operations/key-profiles')) return reply({ data: [profile], total: 1 })
      if (path.includes('/profile-templates?')) return reply({ data: [template, templateB], total: 2 })
      if (path.endsWith('/template-a/export')) return first.promise
      if (path.endsWith('/template-b/export')) return second.promise
      return reply([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Key profiles' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Export Mobile defaults' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Export Desktop defaults' }))
    second.resolve(await reply({ version: 1, name: 'Desktop defaults', profile: templateB.profile }))
    expect(await screen.findByDisplayValue(/"Desktop defaults"/)).toBeInTheDocument()
    await act(async () => { first.resolve(await reply({ version: 1, name: 'Mobile defaults', profile: template.profile })) })
    expect(screen.queryByDisplayValue(/"Mobile defaults"/)).not.toBeInTheDocument()
    expect(screen.getByDisplayValue(/"Desktop defaults"/)).toBeInTheDocument()
  })
})
