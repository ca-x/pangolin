import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import ModelsPage from './ModelsPage'
import { toast } from 'sonner'

vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const paged = (data: unknown[]) => json({ data, total: data.length })

function renderRouting() {
  const requests: unknown[] = []
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([{ id: 'p1', name: 'Project', slug: 'p', enabled: true, is_default: true }])
    if (path.includes('/permissions')) return json(['*'])
    if (path.endsWith('/api-keys')) return json([{ id: 'k1', name: 'QA key', enabled: true }])
    if (path.includes('/routing-preview') && init?.method === 'POST') {
      requests.push(JSON.parse(String(init.body)))
      return json({ candidates: [
        { id: 'candidate-1', provider: 'QA OpenAI', provider_kind: 'openai', upstream_model: 'gpt-4o-mini', endpoint: '/v1/chat/completions', weight: 1 },
        { id: 'candidate-2', provider: 'Backup', upstream_model: 'gpt-4o', endpoint: '/v1/chat/completions', weight: 1 },
      ], decisions: [{ stage: 'association', candidate: null, reason: 'matched' }], estimated_tokens: 16 })
    }
    return paged([])
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/models?tab=routing']}><ProjectProvider><ModelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  return requests
}

describe('routing preview transformation path', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('names the requested model, ordered routing, and first upstream model', async () => {
    const requests = renderRouting()
    await userEvent.type(await screen.findByRole('textbox', { name: 'Requested model' }), 'qa-demo')
    await userEvent.click(screen.getByRole('button', { name: 'Preview' }))

    const heading = await screen.findByRole('heading', { name: 'Model transformation path' })
    const path = heading.closest('.pm-route-journey') as HTMLElement
    expect(path).toHaveAttribute('data-animate', 'true')
    expect(within(path).getByText('qa-demo')).toBeInTheDocument()
    expect(within(path).getByText('2 candidates')).toBeInTheDocument()
    expect(within(path).getByText('gpt-4o-mini')).toBeInTheDocument()
    expect(within(path).getByText('QA OpenAI', { exact: false })).toBeInTheDocument()
    expect(within(path).getByText(/Upstream adapter: OpenAI/)).toBeInTheDocument()
    await waitFor(() => expect(requests).toEqual([{ api_key_id: 'k1', model: 'qa-demo', endpoint: '/v1/chat/completions', body: {} }]))
  })

  it('shows the result immediately for keyboard submission', async () => {
    renderRouting()
    await userEvent.type(await screen.findByRole('textbox', { name: 'Requested model' }), 'qa-demo{Enter}')
    const heading = await screen.findByRole('heading', { name: 'Model transformation path' })
    expect(heading.closest('.pm-route-journey')).toHaveAttribute('data-animate', 'false')
  })

  it('does not let a pending preview from another project block or alert the current project', async () => {
    let rejectFirst: (error: Error) => void = () => { throw new Error('missing request') }
    const first = new Promise<Response>((_resolve, reject) => { rejectFirst = reject })
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([
        { id: 'p1', name: 'Project A', slug: 'a', enabled: true, is_default: true },
        { id: 'p2', name: 'Project B', slug: 'b', enabled: true, is_default: false },
      ])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/api-keys')) return json([{ id: path.includes('/p2/') ? 'k2' : 'k1', name: 'QA key', enabled: true }])
      if (path.includes('/routing-preview') && init?.method === 'POST') return path.includes('/p1/') ? first : json({ candidates: [{ id: 'b', provider: 'Backup', upstream_model: 'upstream-b', endpoint: '/v1/chat/completions' }], decisions: [], estimated_tokens: 1 })
      return paged([])
    }))
    function SwitchProject() {
      const { project, setProjectId } = useProject()
      return <><output data-testid="active-project">{project.id}</output><button onClick={() => setProjectId('p2')}>Switch project</button></>
    }
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/models?tab=routing']}><ProjectProvider><SwitchProject /><ModelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    await userEvent.type(await screen.findByRole('textbox', { name: 'Requested model' }), 'model-a')
    await userEvent.click(screen.getByRole('button', { name: 'Preview' }))
    expect(screen.getByRole('button', { name: 'Preview' })).toBeDisabled()
    await userEvent.click(screen.getByRole('button', { name: 'Switch project' }))
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Preview' })).toBeEnabled())
    await userEvent.type(screen.getByRole('textbox', { name: 'Requested model' }), 'model-b')
    await userEvent.click(screen.getByRole('button', { name: 'Preview' }))
    expect((await screen.findByRole('heading', { name: 'Model transformation path' })).closest('.pm-route-journey')).toHaveTextContent('upstream-b')
    await act(async () => { rejectFirst(new Error('old project failure')); await new Promise((resolve) => setTimeout(resolve, 0)) })
    expect(toast.error).not.toHaveBeenCalledWith('old project failure')
  })
})
