import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import i18n from './i18n'

const response = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
function renderApp(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><App/></MemoryRouter></QueryClientProvider>)
}

describe('application routing', () => {
  beforeEach(() => { void i18n.changeLanguage('en'); vi.restoreAllMocks() })

  it('keeps direct /login outside the authenticated admin shell', async () => {
    vi.stubGlobal('fetch', vi.fn(() => response({ initialized: true, authenticated: false, user: null, capture_payloads: false })))
    renderApp('/login')
    expect(await screen.findByRole('heading', { name: 'Sign in to Pangolin' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument()
  })

  it('redirects initialized anonymous admin routes to /login', async () => {
    vi.stubGlobal('fetch', vi.fn(() => response({ initialized: true, authenticated: false, user: null, capture_payloads: false })))
    renderApp('/channels')
    expect(await screen.findByRole('heading', { name: 'Sign in to Pangolin' })).toBeInTheDocument()
  })

  it('redirects authenticated /login visits into the admin overview', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.includes('bootstrap')) return response({ initialized: true, authenticated: true, user: { id:'owner', email:'owner@example.test', role:'admin', language:'en', theme:'system:bronze', created_at:1 }, capture_payloads:false })
      if (path.endsWith('/projects')) return response([{id:'project-a',name:'Project A',slug:'project-a',owner_user_id:'owner',is_default:true,enabled:true}])
      if (path.includes('/permissions')) return response(['project:read','project:manage','api_key:manage','catalog:manage'])
      if (path.includes('summary')) return response({ requests:0, errors:0, error_rate:null, p95_latency_ms:null, input_tokens:0, output_tokens:0, cost_micros:0, series:[] })
      return response([])
    }))
    renderApp('/login')
    expect(await screen.findByRole('heading', { name: 'Overview' })).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Primary navigation' })).toBeInTheDocument()
    expect(await screen.findByText('No requests in the last 24 hours')).toBeInTheDocument()
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(2)
    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Sign in to Pangolin' })).not.toBeInTheDocument())
  })

  it('routes an uninitialized instance to the distinct setup wizard', async () => {
    vi.stubGlobal('fetch', vi.fn(() => response({ initialized: false, authenticated: false, user: null, capture_payloads: false })))
    renderApp('/login')
    expect(await screen.findByRole('heading', { name: 'Initialize Pangolin' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Sign in to Pangolin' })).not.toBeInTheDocument()
  })

  it('serves the unauthenticated invitation inspect and acceptance flow',async()=>{
    vi.stubGlobal('fetch',vi.fn((input:RequestInfo|URL)=>String(input).includes('invitations/inspect')?response({email:'invitee@example.test',project_id:'p1',role_id:'r1',expires_at:2000000000}):response({initialized:true,authenticated:false,user:null,capture_payloads:false,branding:{instance_name:'Pangolin',branding_name:'Pangolin / 鲮鲤',favicon_url:'/logo.webp',onboarding_complete:false}})))
    renderApp('/invite?token=one-time-token');await userEvent.click(await screen.findByRole('button',{name:'Inspect invitation'}));expect(await screen.findByText(/invitee@example.test/)).toBeInTheDocument();expect(screen.getByLabelText(/Password/)).toHaveAttribute('autocomplete','new-password')
  })

  it('applies bootstrap branding to the document and shell',async()=>{
    vi.stubGlobal('fetch',vi.fn((input:RequestInfo|URL)=>{const path=String(input);if(path.includes('bootstrap'))return response({initialized:true,authenticated:true,user:{id:'u1',email:'owner@example.test',role:'admin',language:'en',theme:'system:bronze',created_at:1},capture_payloads:false,branding:{instance_name:'Meridian',branding_name:'Meridian Gateway',favicon_url:'/brand.ico',onboarding_complete:true}});if(path.endsWith('/projects'))return response([{id:'p1',name:'Project',slug:'project',owner_user_id:'u1',is_default:true,enabled:true}]);if(path.includes('/permissions'))return response(['*']);if(path.includes('summary'))return response({requests:0,errors:0,error_rate:null,p95_latency_ms:null,input_tokens:0,output_tokens:0,cost_micros:0,series:[]});return response({data:[],total:0})}));renderApp('/');expect((await screen.findAllByText('Meridian Gateway')).length).toBeGreaterThan(0);expect(document.title).toBe('Meridian Gateway');expect(document.querySelector<HTMLLinkElement>("link[rel='icon']")?.href).toContain('/brand.ico')
  })
})
