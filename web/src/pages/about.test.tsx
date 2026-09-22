import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import SystemPage, { commitsDiffer, consoleBuild, formatBuildTime } from './SystemPage'

const backendBuild = { version: '9.9.9', commit: 'abcdef123456', built_at: '2026-09-21T04:20:00Z', target: 'x86_64-unknown-linux-gnu', profile: 'release' }
const response = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))

/**
 * The console identity comes from a compile-time `define`, not from a global, so
 * a test cannot stub it with `vi.stubGlobal`. Read the injected value and derive
 * commits from it instead.
 */
function consoleCommit() {
  const bundle = consoleBuild()
  if (!bundle) throw new Error('__WEB_BUILD__ was not injected into the test bundle')
  return bundle.commit
}

const otherCommit = (commit: string) => (commit.replace(/-dirty$/, '') === 'ffffffffffff' ? 'fffffffffffe' : 'ffffffffffff')

function stubBootstrap(build: unknown) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.includes('bootstrap')) return response({ initialized: true, authenticated: true, user: { id: 'owner', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, build })
    // The page reads its permissions from the project context, so the harness
    // supplies one project and the owner scope.
    if (path.endsWith('/projects')) return response([{ id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }])
    if (path.includes('/permissions')) return response(['*'])
    return response({ data: [], total: 0 })
  }))
}

async function openAbout() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><ThemeProvider><MemoryRouter><ProjectProvider><SystemPage /></ProjectProvider></MemoryRouter></ThemeProvider></QueryClientProvider>)
  await userEvent.click(await screen.findByRole('tab', { name: 'About' }))
}

describe('about panel', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.restoreAllMocks() })

  it('reports the backend build from bootstrap in a readable form', async () => {
    stubBootstrap(backendBuild)
    await openAbout()
    expect(await screen.findByRole('heading', { name: 'About' })).toBeInTheDocument()
    expect(screen.getByText('Pangolin / 鲮鲤')).toBeInTheDocument()
    expect(screen.getByText('9.9.9')).toBeInTheDocument()
    expect(screen.getByText('abcdef123456')).toBeInTheDocument()
    expect(screen.getByText('x86_64-unknown-linux-gnu')).toBeInTheDocument()
    expect(screen.getByText('release')).toBeInTheDocument()
    const readable = new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(backendBuild.built_at))
    expect(screen.getByText(readable)).toBeInTheDocument()
    expect(screen.queryByText(backendBuild.built_at)).not.toBeInTheDocument()
    expect(screen.getByText(consoleCommit())).toBeInTheDocument()
  })

  it('shows — for fields the backend did not supply', async () => {
    stubBootstrap({ version: '9.9.9' })
    await openAbout()
    expect(await screen.findByText('9.9.9')).toBeInTheDocument()
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(4)
  })

  it('warns when the backend commit differs from the console commit', async () => {
    stubBootstrap({ ...backendBuild, commit: otherCommit(consoleCommit()) })
    await openAbout()
    expect(await screen.findByText('Console and backend builds differ')).toBeInTheDocument()
    expect(screen.getByText(/Rebuild the web assets/)).toBeInTheDocument()
  })

  it('stays quiet when both builds come from the same revision', async () => {
    stubBootstrap({ ...backendBuild, commit: consoleCommit().replace(/-dirty$/, '') })
    await openAbout()
    expect(await screen.findByText('9.9.9')).toBeInTheDocument()
    expect(screen.queryByText('Console and backend builds differ')).not.toBeInTheDocument()
  })

  it('copies a compact diagnostics block', async () => {
    const writeText = vi.fn((_text: string) => Promise.resolve())
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    stubBootstrap(backendBuild)
    await openAbout()
    await userEvent.click(await screen.findByRole('button', { name: 'Copy diagnostics' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1))
    const block = writeText.mock.calls[0][0]
    expect(block.split('\n')).toEqual([
      'Pangolin / 鲮鲤',
      'backend.version=9.9.9',
      'backend.commit=abcdef123456',
      'backend.built_at=2026-09-21T04:20:00Z',
      'backend.target=x86_64-unknown-linux-gnu',
      'backend.profile=release',
      `console.version=${consoleBuild()!.version}`,
      `console.commit=${consoleCommit()}`,
      `console.builtAt=${consoleBuild()!.builtAt}`,
    ])
  })
})

describe('build identity comparison', () => {
  it('ignores the -dirty suffix and unmeasurable commits', () => {
    expect(commitsDiffer('abc1234def56', 'abc1234def56')).toBe(false)
    expect(commitsDiffer('abc1234def56-dirty', 'abc1234def56')).toBe(false)
    expect(commitsDiffer('abc1234def56', 'def5678abc90')).toBe(true)
    expect(commitsDiffer('abc1234def56', '')).toBe(false)
    expect(commitsDiffer(undefined, 'abc1234def56')).toBe(false)
    expect(commitsDiffer('unknown', 'abc1234def56')).toBe(false)
  })

  it('renders an unparsable build time verbatim and a missing one as —', () => {
    expect(formatBuildTime('', 'en')).toBe('—')
    expect(formatBuildTime(undefined, 'en')).toBe('—')
    expect(formatBuildTime('unknown', 'en')).toBe('unknown')
    expect(formatBuildTime('2026-09-21T04:20:00Z', 'en')).toBe(new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date('2026-09-21T04:20:00Z')))
  })
})
