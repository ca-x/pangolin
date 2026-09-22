import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import SystemPage from './SystemPage'
import { ThemeProvider } from '../theme'

/**
 * The webhook form's body template defaulted to the bare string `$event`. A JSON
 * field stringifies only when its value is an object (`shared.tsx:307`), so the
 * editor showed `$event` and the submit's `JSON.parse` (`:102`) threw — the field's
 * own default was invalid for its own parser. The backend renders a template's
 * `body` member (`src/operations/runtime.rs:585-606`), so the default is the object
 * it can actually render.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }

const mockApi = () => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('settings/orchestration')) return json({ version: 1, affinity_rules: [], session_compaction: {} })
  if (path.includes('/projects')) return json([project])
  return json({ data: [], total: 0 })
})

const renderPage = (fetchMock: ReturnType<typeof vi.fn>) => {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><ThemeProvider><MemoryRouter><ProjectProvider><SystemPage /></ProjectProvider></MemoryRouter></ThemeProvider></QueryClientProvider>)
}

describe('the webhook body template starts from a document the form can submit', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('opens with valid JSON that names the event the backend renders', async () => {
    renderPage(mockApi())
    await userEvent.click(await screen.findByRole('tab', { name: 'Webhooks' }))
    await userEvent.click(await screen.findByRole('button', { name: /Add webhook/ }))

    const dialog = await screen.findByRole('dialog')
    // The template editor is collapsed until it is opened.
    await userEvent.click(within(dialog).getByRole('button', { name: /Body template/ }))
    const editor = within(dialog).getByLabelText('Body template') as HTMLTextAreaElement

    expect(() => JSON.parse(editor.value)).not.toThrow()
    expect(JSON.parse(editor.value)).toEqual({ body: '$event' })
  })
})
