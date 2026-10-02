import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, expect, it } from 'vitest'
import i18n from '../i18n'
import ChannelsPage from './ChannelsPage'
import ModelsPage from './ModelsPage'
import { json, mockApi, models, mount } from './referenceConsoleFixtures'
beforeEach(async () => { localStorage.clear(); await i18n.changeLanguage('en') })
it.each(['en', 'zh-CN'])('shows known, stale and unknown inventory without fingerprints in %s', async (language) => {
  await i18n.changeLanguage(language)
  mount(<ChannelsPage />, '/channels?tab=credentials', mockApi())
  const table = within(await screen.findByRole('region', { name: i18n.t('credentials') }))
  expect(table.getByText(i18n.t('inventoryKnown'))).toBeInTheDocument()
  expect(table.getByText(i18n.t('inventoryStale'))).toBeInTheDocument()
  expect(table.getAllByText(i18n.t('inventoryUnknown')).length).toBeGreaterThan(0)
  const known = within(table.getByText('1111').closest('tr')!)
  expect(known.getByText('0')).toBeInTheDocument()
  const unknown = within(table.getByText('3333').closest('tr')!)
  expect(unknown.getAllByText('—').length).toBeGreaterThan(1)
  const mobile = within(document.querySelector('.mobile-resource-list') as HTMLElement)
  expect(mobile.getByText(i18n.t('inventoryKnown'))).toBeInTheDocument()
  expect(mobile.getAllByText(i18n.t('inventoryLastSuccess')).length).toBeGreaterThan(0)
})
it.each(['en', 'zh-CN'])('distinguishes false, unknown and empty shared metadata in %s', async (language) => {
  await i18n.changeLanguage(language)
  const model = { ...models[0], catalog_metadata: { capabilities: { tools: false, streaming: true, vision: null }, limits: { context: 8192, output: null }, modalities: { input: ['text'], output: [] }, reasoning_levels: null } }
  mount(<ModelsPage />, '/models', mockApi((path) => path.includes('/operations/models') ? json({ data: [model], total: 1 }) : undefined))
  const region = within(await screen.findByRole('region', { name: i18n.t('models') }))
  await userEvent.click(region.getByRole('button', { name: `${i18n.t('modelFacts')} Fast` }))
  const dialog = within(await screen.findByRole('dialog'))
  expect(dialog.getByText(i18n.t('modelConditionalRoutes'))).toBeInTheDocument()
  expect(dialog.getByText(i18n.t('factUnsupported'))).toBeInTheDocument()
  expect(dialog.getAllByText(i18n.t('factUnknown')).length).toBeGreaterThan(1)
  expect(dialog.getByText(i18n.t('factNone'))).toBeInTheDocument()
  expect(dialog.getByText('8,192')).toBeInTheDocument()
})
it('offers retry on inventory query failure and a useful empty result', async () => {
  let failed = true
  mount(<ChannelsPage />, '/channels?tab=credentials', mockApi((path) => path.includes('/operations/credentials') ? json(failed ? { error: { message: 'failed' } } : { data: [], total: 0 }, failed ? 503 : 200) : undefined))
  await screen.findByText(i18n.t('networkError'))
  failed = false
  await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
  expect(await screen.findByText(i18n.t('credentialEmpty'))).toBeInTheDocument()
})
