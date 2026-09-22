import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import Shell from './Shell'
import { ONBOARDING_DISMISSED_KEY } from './onboarding'
import SystemPage from './pages/SystemPage'
import { ThemeProvider } from './theme'

/**
 * The first-run guidance could only be turned off by flipping the instance-wide
 * `onboarding_complete` branding switch, which declares onboarding finished for
 * everyone. It is now dismissible in place, remembered per instance in the
 * browser that dismissed it, and restorable from the appearance settings — so an
 * operator who only wants the banner gone does not have to answer for every
 * other user, and one who wants it back can ask for it.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const user = { id: 'u1', email: 'principal@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }
const branding = (instance: string) => ({ instance_name: instance, branding_name: instance, favicon_url: '/logo.webp', onboarding_complete: false })

const renderShell = (instance = 'Pangolin') => {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('/settings/system')) return json(branding(instance))
    return json({ data: [], total: 0 })
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <MemoryRouter initialEntries={['/system']}>
          <Routes>
            <Route element={<Shell user={user} branding={branding(instance)} />}>
              <Route path="/system" element={<SystemPage />} />
            </Route>
          </Routes>
        </MemoryRouter>
      </ThemeProvider>
    </QueryClientProvider>,
  )
}

describe('the first-run guidance is dismissible', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('hides the guidance in place and remembers the choice for this instance', async () => {
    const first = renderShell()
    expect(await screen.findByText('Add a channel, credential, and model route before sending requests.')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    expect(screen.queryByText('Add a channel, credential, and model route before sending requests.')).not.toBeInTheDocument()
    expect(localStorage.getItem(ONBOARDING_DISMISSED_KEY)).toBe('Pangolin')
    first.unmount()

    // The same instance stays dismissed, and a different instance is not
    // affected by a dismissal recorded somewhere else.
    const second = renderShell('Pangolin')
    expect(await screen.findByRole('link', { name: 'Overview' })).toBeInTheDocument()
    expect(screen.queryByText('Add a channel, credential, and model route before sending requests.')).not.toBeInTheDocument()

    second.unmount()
    renderShell('Other instance')
    expect(await screen.findByText('Add a channel, credential, and model route before sending requests.')).toBeInTheDocument()
  })

  it('can be brought back from the appearance settings', async () => {
    renderShell()
    await userEvent.click(await screen.findByRole('button', { name: 'Dismiss' }))
    expect(screen.queryByText('Add a channel, credential, and model route before sending requests.')).not.toBeInTheDocument()

    const restore = await screen.findByRole('button', { name: 'Show it again' })
    await userEvent.click(restore)
    expect(localStorage.getItem(ONBOARDING_DISMISSED_KEY)).toBeNull()
    expect(await screen.findByText('Add a channel, credential, and model route before sending requests.')).toBeInTheDocument()
  })

  /**
   * At 375px the banner was a squeezed two-column row: the explanation broke
   * almost word by word beside the action, which kept its intrinsic width. The
   * stacked layout that fixes it lives in `styles.css` (a jsdom render cannot
   * measure layout), so what is pinned here is the block it stacks: one copy
   * line, then one action and one named dismiss control.
   */
  it('keeps one action and one named dismiss control in the banner row', async () => {
    renderShell()
    const hint = await screen.findByText('Add a channel, credential, and model route before sending requests.')
    const row = hint.closest('.pm-onboarding-row') as HTMLElement
    expect(row).not.toBeNull()
    expect(row.children).toHaveLength(2)
    const controls = row.lastElementChild as HTMLElement
    expect(within(controls).getAllByRole('link')).toHaveLength(1)
    expect(within(controls).getByRole('button', { name: 'Dismiss' })).toBeInTheDocument()
  })
})
