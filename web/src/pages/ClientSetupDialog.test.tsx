import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ClientSetupDialog } from './ClientSetupDialog'

const response = (models: unknown[], status = 200) => Promise.resolve(new Response(JSON.stringify({ models }), { status, headers: { 'Content-Type': 'application/json' } }))
const properties = { projectId: 'project-a', keyId: 'key-a', keyName: 'Key A', onClose: vi.fn() }
const wrapper = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return ({ children }: { children: React.ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}
beforeEach(() => { void i18n.changeLanguage('en'); vi.stubGlobal('fetch', vi.fn(() => response([{ id: 'public', metadata: { reasoning_levels: ['low', 'high'] } }]))); properties.onClose.mockReset() })
describe('client setup dialog', () => {
  it('requests real streaming endpoint eligibility and uses existing-key environment placeholder', async () => {
    render(<ClientSetupDialog {...properties} />, { wrapper: wrapper() })
    expect(await screen.findByText(/wire_api = "responses"/)).toHaveTextContent('PANGOLIN_API_KEY')
    expect(screen.getByRole('tabpanel', { name: 'Codex' })).toBeInTheDocument()
    expect(String(vi.mocked(fetch).mock.calls[0][0])).toContain('endpoint=%2Fv1%2Fresponses&stream=true')
    await userEvent.click(screen.getByRole('tab', { name: 'Claude Code' }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([path]) => String(path).includes('endpoint=%2Fv1%2Fmessages&stream=true'))).toBe(true))
    expect(await screen.findByText(/ANTHROPIC_BASE_URL/)).not.toHaveTextContent('/v1/v1')
    expect(screen.getByRole('tabpanel', { name: 'Claude Code' })).toBeInTheDocument()
  })
  it('shows loading, failed query retry, and an honest empty state', async () => {
    let finish!: (value: Response) => void
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>((resolve) => { finish = resolve })))
    render(<ClientSetupDialog {...properties} />, { wrapper: wrapper() })
    expect(screen.getByText('Loading eligible models…')).toBeInTheDocument()
    finish(new Response('{}', { status: 500, headers: { 'Content-Type': 'application/json' } }))
    expect(await screen.findByText('Could not read client models.')).toBeInTheDocument()
    vi.stubGlobal('fetch', vi.fn(() => response([])))
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(await screen.findByText(/No models currently support this client/)).toBeInTheDocument()
    expect(screen.queryByText(/wire_api/)).not.toBeInTheDocument()
  })
  it('removes token and code immediately on close and clears context on project change', async () => {
    const view = render(<ClientSetupDialog {...properties} token="one-time-secret" />, { wrapper: wrapper() })
    expect(await screen.findByText(/wire_api/)).toHaveTextContent('one-time-secret')
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(properties.onClose).toHaveBeenCalledOnce()
    expect(screen.queryByText(/one-time-secret/)).not.toBeInTheDocument()
    view.rerender(<ClientSetupDialog {...properties} projectId="project-b" token="one-time-secret" />)
    expect(screen.queryByText(/one-time-secret/)).not.toBeInTheDocument()
  })
  it.each(['en', 'zh-CN'])('localizes state and validates base address in %s', async (language) => {
    await i18n.changeLanguage(language)
    render(<ClientSetupDialog {...properties} />, { wrapper: wrapper() })
    await screen.findByText(/wire_api/)
    const base = screen.getByLabelText(i18n.t('clientGatewayBase'))
    fireEvent.change(base, { target: { value: 'file:///tmp/a' } })
    expect(screen.getByText(i18n.t('clientBaseInvalid'))).toBeInTheDocument()
    expect(screen.queryByText(/wire_api/)).not.toBeInTheDocument()
  })
})
