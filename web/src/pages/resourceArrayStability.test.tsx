import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ResourcePage } from './shared'

const tableData = vi.hoisted(() => [] as unknown[][])
vi.mock('@tanstack/react-table', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-table')>()
  return {
    ...actual,
    useReactTable: (options: Parameters<typeof actual.useReactTable>[0]) => {
      tableData.push(options.data)
      return actual.useReactTable(options)
    },
  }
})

const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const response = (body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { 'Content-Type': 'application/json' } }))

beforeEach(async () => { await i18n.changeLanguage('en'); tableData.length = 0 })

it('keeps bare-array table data stable while a project edit dialog opens', async () => {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.includes('/permissions')) return response(['*'])
    if (path.includes('/projects')) return response([project])
    return response([])
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider>
    <ResourcePage resource="projects" endpoint="/api/admin/v1/projects" title="Projects" description="Projects" empty="None" columns={[{ key: 'name', label: 'Name' }]} fields={[{ key: 'name', label: 'Name', required: true }]} />
  </ProjectProvider></MemoryRouter></QueryClientProvider>)

  const table = await screen.findByRole('region', { name: 'Projects' })
  const rowsBeforeEdit = [...tableData].reverse().find((data) => data.length === 1)
  expect(rowsBeforeEdit).toBeDefined()
  await userEvent.click(within(table).getByRole('button', { name: 'Edit' }))
  expect(screen.getByRole('dialog', { name: 'Edit Projects' })).toBeInTheDocument()
  expect(tableData.at(-1)).toBe(rowsBeforeEdit)
})
