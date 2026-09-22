import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { SkeletonRows } from '../components'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import AccessPage from './AccessPage'

/**
 * Every visible string is localized in both languages. These are the leaks the
 * console shipped: English labels and enum values rendered verbatim inside an
 * otherwise translated page, and an accessible name that stayed English.
 */
const source = (name: string) => readFileSync(join(process.cwd(), 'src', name), 'utf8')

describe('localization leaks', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('routes the flagged labels and placeholders through the locale files', () => {
    const leaks: Array<[string, RegExp]> = [
      ['pages/ModelsPage.tsx', /label="ID"/],
      ['pages/ModelsPage.tsx', /label="HTTPS URL"/],
      ['pages/ChannelsPage.tsx', /placeholder="id-1, id-2"/],
      ['pages/AccessPage.tsx', /label: 'active'/],
      ['pages/AccessPage.tsx', /label: 'suspended'/],
      ['components.tsx', /aria-label="Loading"/],
    ]
    for (const [file, pattern] of leaks) {
      expect([file, pattern.source, pattern.test(source(file))]).toEqual([file, pattern.source, false])
    }
  })

  it('resolves the replacement keys in both languages', () => {
    for (const key of ['identifier', 'httpsUrl', 'resourceIdsPlaceholder', 'loading', 'matchExact', 'matchRegex', 'matchTag']) {
      for (const language of ['en', 'zh-CN']) {
        expect([key, language, i18n.exists(key, { lng: language })]).toEqual([key, language, true])
      }
    }
  })

  it('names the loading skeleton in the active language', async () => {
    render(<SkeletonRows count={1} />)
    expect(screen.getByLabelText(i18n.t('loading', { lng: 'en' }))).toBeInTheDocument()
    await i18n.changeLanguage('zh-CN')
    expect(screen.getByLabelText(i18n.t('loading', { lng: 'zh-CN' }))).toBeInTheDocument()
    expect(i18n.t('loading', { lng: 'en' })).not.toBe(i18n.t('loading', { lng: 'zh-CN' }))
    await i18n.changeLanguage('en')
  })

  it('translates the membership status values instead of showing the raw enum', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      const data = path.endsWith('/projects') ? [{ id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }] : path.includes('/permissions') ? ['*'] : []
      return Promise.resolve(new Response(JSON.stringify(data), { status: 200, headers: { 'Content-Type': 'application/json' } }))
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    await userEvent.click(await screen.findByRole('tab', { name: 'Roles' }))
    await userEvent.click(await screen.findByRole('button', { name: i18n.t('assignments', { lng: 'en' }) }))
    await userEvent.click(await screen.findByRole('combobox', { name: 'Status' }))
    expect(await screen.findByRole('option', { name: i18n.t('memberStatusActive', { lng: 'en' }) })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: i18n.t('memberStatusSuspended', { lng: 'en' }) })).toBeInTheDocument()
    await userEvent.keyboard('{Escape}')

    await i18n.changeLanguage('zh-CN')
    await userEvent.click(await screen.findByRole('combobox', { name: i18n.t('status', { lng: 'zh-CN' }) }))
    expect(await screen.findByRole('option', { name: i18n.t('memberStatusActive', { lng: 'zh-CN' }) })).toBeInTheDocument()
    await i18n.changeLanguage('en')
  })
})
