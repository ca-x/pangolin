import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import AccessPage from './AccessPage'

describe('API key administration', () => {
  beforeEach(() => { void i18n.changeLanguage('en'); vi.stubGlobal('fetch', vi.fn((input:RequestInfo|URL) => { const path=String(input); const data=path.endsWith('/projects')||path.includes('/projects?')?[{id:'project-a',name:'Project A',slug:'project-a',owner_user_id:'owner',is_default:true,enabled:true}]:path.includes('/permissions')?['project:read','api_key:manage']:[]; return Promise.resolve(new Response(JSON.stringify(data), { status: 200, headers: { 'Content-Type': 'application/json' } })) })) })
  it('explains generated and one-time imported token modes', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage/></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('button', { name: 'Create API key' }))
    expect(screen.getByText(/shown once/i)).toBeInTheDocument()
    await userEvent.click(screen.getByRole('combobox', { name: 'Token mode' }))
    await userEvent.click(screen.getByRole('option', { name: 'Import existing' }))
    expect(screen.getByText(/stores only an indexed digest and Argon2id/i)).toBeInTheDocument()
    expect(screen.getByLabelText(/Existing token/)).toHaveAttribute('autocomplete', 'off')
  })

  it('uses the selected project and the real PATCH endpoint for project updates', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage/></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Projects' }))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    const name=screen.getByLabelText('Name');await userEvent.clear(name);await userEvent.type(name,'Renamed project')
    await userEvent.click(screen.getByRole('button',{name:'Save'}))
    await vi.waitFor(()=>expect(vi.mocked(fetch).mock.calls.some(([path,init])=>String(path).endsWith('/projects/project-a')&&init?.method==='PATCH'&&new Headers(init.headers).get('X-Pangolin-CSRF')==='1')).toBe(true))
  })
})
