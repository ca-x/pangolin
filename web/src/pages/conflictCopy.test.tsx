import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost } from '../components'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ResourcePage } from './shared'

/**
 * A refused delete answered with an English sentence inside a Chinese console: the
 * API sends code `conflict_error` with the detail in the message, and the console
 * printed the message verbatim (`web/src/components.tsx`). The console cannot name
 * the code in the active language without two changes it did not have — a specific
 * code on the conflict, and `api()` keeping it — so this pins all three parts: the
 * localized sentence, and the server's detail kept beside it, because the detail is
 * where the colliding value lives.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const channel = { id: 'c1', name: 'openai-prod', enabled: true }

const mockApi = () => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
  const path = String(input)
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (init?.method === 'DELETE') return json({ error: { type: 'resource_in_use', message: 'this channel still has models or credentials' } }, 409)
  return json({ data: [channel], total: 1 })
})

const renderPage = () => {
  vi.stubGlobal('fetch', mockApi())
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ProjectProvider>
          <ResourcePage resource="channels" title="Channels" description="" empty="" columns={[{ key: 'name', label: 'Name' }]} fields={[{ key: 'name', label: 'Name' }]} />
          <ConfirmHost />
        </ProjectProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('a refused delete is explained in the operator\'s language', () => {
  beforeEach(async () => { await i18n.changeLanguage('zh-CN'); vi.clearAllMocks() })

  it('names the reason in Chinese and keeps the server detail', async () => {
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /删除 openai-prod/ }))
    await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '删除' }))

    const dialog = await screen.findByRole('dialog')
    const alert = await within(dialog).findByRole('alert')
    expect(alert).toHaveTextContent('该通道仍有模型或凭据')
    // The detail carries the colliding value, so it stays.
    expect(alert).toHaveTextContent('this channel still has models or credentials')
  })

  it('still shows a server message it has no code for', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (init?.method === 'DELETE') return json({ error: { type: 'conflict_error', message: 'a record with those values already exists' } }, 409)
      return json({ data: [channel], total: 1 })
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ProjectProvider>
            <ResourcePage resource="channels" title="Channels" description="" empty="" columns={[{ key: 'name', label: 'Name' }]} fields={[{ key: 'name', label: 'Name' }]} />
            <ConfirmHost />
          </ProjectProvider>
        </MemoryRouter>
      </QueryClientProvider>,
    )
    await userEvent.click(await screen.findByRole('button', { name: /删除 openai-prod/ }))
    await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '删除' }))

    const alert = await within(await screen.findByRole('dialog')).findByRole('alert')
    expect(alert).toHaveTextContent('a record with those values already exists')
  })
})
