import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Login, OidcLinkPanel, buttonTextColor } from './Auth'
import i18n from './i18n'

const response = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), {
  status,
  headers: { 'Content-Type': 'application/json' },
}))

function renderLogin() {
  return render(<MemoryRouter><Login /></MemoryRouter>)
}

describe('OIDC sign-in discovery', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.restoreAllMocks()
  })

  it('renders every provider and follows the encoded server start response', async () => {
    const assign = vi.fn()
    vi.stubGlobal('location', { origin: 'https://pangolin.test', assign, reload: vi.fn() })
    const fetch = vi.fn((input: RequestInfo | URL) => String(input).includes('/company-sso/start')
      ? response({ authorization_url: 'https://identity.example/authorize?state=one' })
      : response([
          { id: 'company-sso', display_name: 'Company SSO', button_color: '#FFFFFF', logo_key: 'lobehub:Github', login_only: false },
          { id: 'partner/sso ?blue', display_name: 'A very long partner identity provider name that must remain readable', button_color: '#111111', logo_key: 'catalog:missing', login_only: true },
        ]))
    vi.stubGlobal('fetch', fetch)

    renderLogin()

    await userEvent.click(await screen.findByRole('button', { name: 'Continue with Company SSO' }))
    await waitFor(() => expect(fetch).toHaveBeenCalledWith('/api/v1/auth/oidc/company-sso/start', expect.objectContaining({ credentials: 'same-origin' })))
    expect(assign).toHaveBeenCalledWith('https://identity.example/authorize?state=one')
    const fallback = screen.getByRole('button', { name: 'Continue with A very long partner identity provider name that must remain readable' })
    expect(fallback).toBeInTheDocument()
    expect(fallback).toHaveTextContent('AV')
    expect(fallback).toHaveStyle({ backgroundColor: '#111111', color: '#FFFFFF' })
    expect(fetch).toHaveBeenCalledWith('/api/v1/auth/oidc/providers', expect.objectContaining({ credentials: 'same-origin' }))
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled()
  })

  it('keeps password login while loading then hides it only after successful provider-only discovery', async () => {
    let resolveDiscovery: (value: Response) => void = () => undefined
    const discovery = new Promise<Response>((resolve) => { resolveDiscovery = resolve })
    vi.stubGlobal('fetch', vi.fn(() => discovery))

    renderLogin()
    expect(screen.getByLabelText(/Email/)).toBeEnabled()
    expect(screen.getByLabelText(/Password/)).toBeEnabled()

    resolveDiscovery(await response([
      { id: 'workforce', display_name: 'Workforce', button_color: null, logo_key: null, login_only: true },
      { id: 'partners', display_name: 'Partners', button_color: '#0B5A46', logo_key: 'catalog:unknown', login_only: true },
    ]))
    await waitFor(() => expect(screen.queryByLabelText(/Email/)).not.toBeInTheDocument())
    expect(screen.queryByLabelText(/Password/)).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Sign in' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Continue with Workforce' })).toBeEnabled()
  })

  it('leaves only the password sign-in form when discovery is empty', async () => {
    const fetch = vi.fn(() => response([]))
    vi.stubGlobal('fetch', fetch)

    renderLogin()

    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1))
    expect(screen.queryByRole('button', { name: /Continue with/ })).not.toBeInTheDocument()
    expect(screen.getByLabelText(/Email/)).toBeEnabled()
    expect(screen.getByLabelText(/Password/)).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled()
  })

  it('silently preserves password sign-in when discovery fails', async () => {
    const fetch = vi.fn(() => response({ error: { message: 'unavailable' } }, 503))
    vi.stubGlobal('fetch', fetch)

    renderLogin()

    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1))
    expect(screen.getByLabelText(/Email/)).toBeEnabled()
    expect(screen.getByLabelText(/Password/)).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('provides localized accessible provider labels in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn(() => response([{ id: 'workforce', display_name: '企业身份中心', button_color: null, logo_key: null, login_only: false }])))

    renderLogin()

    expect(await screen.findByRole('button', { name: '使用企业身份中心继续' })).toBeEnabled()
  })

  it('keeps the provider choices and reports a start failure inline', async () => {
    const fetch = vi.fn((input: RequestInfo | URL) => String(input).includes('/workforce/start')
      ? response({ error: { message: 'OIDC discovery failed.' } }, 502)
      : response([{ id: 'workforce', display_name: 'Workforce', button_color: null, logo_key: null, login_only: false }]))
    vi.stubGlobal('fetch', fetch)
    renderLogin()

    await userEvent.click(await screen.findByRole('button', { name: 'Continue with Workforce' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('OIDC discovery failed.')
    expect(screen.getByRole('button', { name: 'Continue with Workforce' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled()
  })
})

describe('OIDC provider button contrast', () => {
  it('chooses the higher-contrast black or white text for valid colors', () => {
    expect(buttonTextColor('#FFFFFF')).toBe('#000000')
    expect(buttonTextColor('#000000')).toBe('#FFFFFF')
    expect(buttonTextColor('#777777')).toBe('#000000')
    expect(buttonTextColor('#0B5A46')).toBe('#FFFFFF')
    expect(buttonTextColor('https://unsafe.example/color')).toBeUndefined()
  })
})

describe('signed-in OIDC linking', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.restoreAllMocks() })

  it('posts with CSRF through the API helper and navigates only to HTTP(S)', async () => {
    const navigate = vi.fn()
    const fetch = vi.fn((input: RequestInfo | URL) => String(input).endsWith('/providers')
      ? response([{ id: 'workforce/sso', display_name: 'Workforce', button_color: null, logo_key: null, login_only: false }])
      : response({ authorization_url: 'https://identity.example.test/authorize?state=opaque' }))
    vi.stubGlobal('fetch', fetch)
    render(<MemoryRouter><OidcLinkPanel navigate={navigate} /></MemoryRouter>)

    await userEvent.click(await screen.findByRole('button', { name: 'Link Workforce' }))
    expect(fetch).toHaveBeenCalledWith('/api/v1/auth/oidc/workforce%2Fsso/link/start', expect.objectContaining({
      method: 'POST',
      headers: expect.objectContaining({ 'X-Pangolin-CSRF': '1' }),
    }))
    expect(navigate).toHaveBeenCalledWith('https://identity.example.test/authorize?state=opaque')
  })

  it('shows an explicit failure and does not navigate for an unsafe start response', async () => {
    const navigate = vi.fn()
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => String(input).endsWith('/providers')
      ? response([{ id: 'workforce', display_name: 'Workforce', button_color: null, logo_key: null, login_only: false }])
      : response({ authorization_url: 'javascript:alert(1)' })))
    render(<MemoryRouter><OidcLinkPanel navigate={navigate} /></MemoryRouter>)

    await userEvent.click(await screen.findByRole('button', { name: 'Link Workforce' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid sign-in address')
    expect(navigate).not.toHaveBeenCalled()
  })
})
