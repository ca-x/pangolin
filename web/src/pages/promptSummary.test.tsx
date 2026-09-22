import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import PromptsPage from './PromptsPage'

/**
 * The prompt list returns `activation` and `enabled`, and the table showed only
 * the name, role, content and status, so what a prompt does was visible only by
 * opening its JSON editor. The summary names the combination, counts the
 * conditions and lists the request fields they read — the shape
 * `src/orchestration/policy.rs::evaluate` accepts: `{version, all|any: [...]}`
 * over `{field, op, value}` leaves, where a version-only document matches
 * everything.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const paged = (rows: unknown[]) => json({ data: rows, total: rows.length, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const prompt = (overrides: Record<string, unknown>) => ({ id: 'p', name: 'prompt', role: 'system', content: 'x', activation: { version: 1 }, order: 0, action: 'prepend', enabled: true, ...overrides })
const prompts = [
  prompt({ id: 'p1', name: 'Always prompt', content: 'be brief' }),
  prompt({ id: 'p2', name: 'All prompt', activation: { version: 1, all: [{ field: '/body/model', op: 'eq', value: 'gpt-4o' }, { field: '/endpoint', op: 'eq', value: '/v1/chat/completions' }] } }),
  prompt({ id: 'p3', name: 'Any prompt', activation: { version: 1, any: [{ field: '/body/model', op: 'eq', value: 'm' }] }, order: 20, action: 'append', enabled: false }),
  prompt({ id: 'p4', name: 'Nested prompt', activation: { version: 1, all: [{ field: '/body/model', op: 'eq', value: 'm' }, { version: 1, any: [{ field: '/headers/x-tenant', op: 'eq', value: 't' }] }] } }),
  // A document this console does not recognize, and one that was never set:
  // neither is presented as a working condition.
  prompt({ id: 'p5', name: 'Opaque prompt', activation: { version: 1, surprise: true } }),
  prompt({ id: 'p6', name: 'No activation', content: 'y', activation: null }),
]

describe('the prompt list says what a prompt does', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('summarizes the conditions and names their fields', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/prompts')) return paged(prompts)
      return paged([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PromptsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    expect(await screen.findByRole('columnheader', { name: 'Activation' })).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Order' })).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Action' })).toBeInTheDocument()

    const always = screen.getByRole('row', { name: /Always prompt/ })
    expect(within(always).getByText('Always applies')).toBeInTheDocument()

    const all = screen.getByRole('row', { name: /All prompt/ })
    expect(within(all).getByText('All conditions (2)')).toBeInTheDocument()
    expect(within(all).getByText('Fields: /body/model, /endpoint')).toBeInTheDocument()

    const any = screen.getByRole('row', { name: /Any prompt/ })
    expect(within(any).getByText('Any condition (1)')).toBeInTheDocument()
    expect(within(any).getByText('20')).toBeInTheDocument()
    expect(within(any).getByText('Append')).toBeInTheDocument()
    // The status column still reports enablement next to the conditions.
    expect(within(any).getByText('Disabled')).toBeInTheDocument()

    const nested = screen.getByRole('row', { name: /Nested prompt/ })
    expect(within(nested).getByText('All conditions (2)')).toBeInTheDocument()
    expect(within(nested).getByText('Fields: /body/model, /headers/x-tenant')).toBeInTheDocument()

    expect(within(screen.getByRole('row', { name: /Opaque prompt/ })).getByText('Unrecognized conditions')).toBeInTheDocument()
    expect(within(screen.getByRole('row', { name: /No activation/ })).getByText('—')).toBeInTheDocument()
  })

  it('creates prompts disabled with explicit placement defaults', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/prompts')) return paged(prompts)
      return paged([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PromptsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    await userEvent.click(await screen.findByRole('button', { name: 'Add prompt' }))
    const dialog = screen.getByRole('dialog', { name: 'Add prompt' })
    // Mantine NumberInput is a decimal text input and Select exposes its label,
    // while Switch has its own ARIA role.
    expect(within(dialog).getByRole('textbox', { name: 'Order' })).toHaveValue('0')
    expect(within(dialog).getByRole('combobox', { name: 'Action' })).toHaveValue('Prepend')
    expect(within(dialog).getByRole('switch', { name: 'Enabled' })).not.toBeChecked()
  })

  it('localizes the placement columns and values in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/prompts')) return paged(prompts)
      return paged([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PromptsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    expect(await screen.findByRole('columnheader', { name: '顺序' })).toBeInTheDocument()
    const any = screen.getByRole('row', { name: /Any prompt/ })
    expect(within(any).getByText('追加')).toBeInTheDocument()
  })

  it('shows protection metadata and previews every rule with an explicit loading state', async () => {
    const rules = [
      { id: 'r1', name: 'Secrets', description: 'Masks numbered secrets', content_pattern: 'secret-[0-9]+', action: 'redact', replacement: '[MASKED]', scopes: { version: 1 }, test_mode: false, enabled: true, state: 'active' },
      { id: 'r2', name: 'Legacy', description: 'Kept for reference', content_pattern: 'legacy', action: 'redact', replacement: '[OLD]', scopes: { version: 1 }, test_mode: false, enabled: true, state: 'archived' },
    ]
    let finishPreview: ((response: Response) => void) | undefined
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/prompts')) return paged(prompts)
      if (path.includes('/operations/protection')) return paged(rules)
      if (path.includes('/protection-preview')) return new Promise<Response>((resolve) => { finishPreview = resolve })
      return paged([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PromptsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    await userEvent.click(await screen.findByRole('tab', { name: 'Protection' }))
    const legacy = await screen.findByRole('row', { name: /Legacy/ })
    expect(within(legacy).getByText('Kept for reference')).toBeInTheDocument()
    expect(within(legacy).getByText('Archived')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: 'Preview rules' }))
    const dialog = screen.getByRole('dialog', { name: 'Preview protection rules' })
    const sample = within(dialog).getByRole('textbox', { name: 'Sample text' })
    const run = within(dialog).getByRole('button', { name: 'Run preview' })
    fireEvent.change(sample, { target: { value: '密'.repeat(6_000) } })
    expect(within(dialog).getByText('Sample text must be 16 KiB or less.')).toBeInTheDocument()
    expect(run).toBeDisabled()
    fireEvent.change(sample, { target: { value: '' } })
    await userEvent.type(sample, 'secret-123 legacy')
    await userEvent.click(run)
    expect(await within(dialog).findByRole('status')).toHaveTextContent('Previewing rules…')
    finishPreview?.(await json({ rules: [
      { id: 'r1', name: 'Secrets', description: 'Masks numbered secrets', action: 'redact', enabled: true, state: 'active', matched: true, result: '[MASKED] legacy' },
      { id: 'r2', name: 'Legacy', description: 'Kept for reference', action: 'redact', enabled: true, state: 'archived', matched: true, result: 'secret-123 [OLD]' },
    ] }))

    expect(await within(dialog).findByText('[MASKED] legacy')).toBeInTheDocument()
    expect(within(dialog).getByText('secret-123 [OLD]')).toBeInTheDocument()
    expect(within(dialog).getAllByText('Matched')).toHaveLength(2)
    expect(within(dialog).getByText('Archived')).toBeInTheDocument()

    await userEvent.click(within(dialog).getByRole('button', { name: 'Close' }))
    await userEvent.click(screen.getByRole('button', { name: 'Add rule' }))
    const form = screen.getByRole('dialog', { name: 'Add rule' })
    expect(within(form).getByRole('textbox', { name: 'Description' })).toBeInTheDocument()
    expect(within(form).getByRole('combobox', { name: 'Rule state' })).toHaveValue('Active')
  })

  it('localizes preview errors, retries the same sample, and names an empty result', async () => {
    await i18n.changeLanguage('zh-CN')
    let previews = 0
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/prompts')) return paged(prompts)
      if (path.includes('/operations/protection')) return paged([{ id: 'r1', name: '规则', description: '', content_pattern: 'secret', action: 'deny', test_mode: false, enabled: true, state: 'active' }])
      if (path.includes('/protection-preview')) {
        previews += 1
        return previews === 1
          ? Promise.resolve(new Response(JSON.stringify({ error: { message: 'preview unavailable' } }), { status: 500, headers: { 'Content-Type': 'application/json' } }))
          : json({ rules: [] })
      }
      return paged([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PromptsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    await userEvent.click(await screen.findByRole('tab', { name: '防护规则' }))
    await userEvent.click(await screen.findByRole('button', { name: '预览规则' }))
    const dialog = screen.getByRole('dialog', { name: '预览防护规则' })
    await userEvent.type(within(dialog).getByRole('textbox', { name: '示例文本' }), 'secret')
    await userEvent.click(within(dialog).getByRole('button', { name: '运行预览' }))
    expect(await within(dialog).findByText('preview unavailable')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: '重试' }))
    await waitFor(() => expect(previews).toBe(2))
    expect(await within(dialog).findByText('没有可预览的防护规则。')).toBeInTheDocument()
  })
})
