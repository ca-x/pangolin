import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, expect, it } from 'vitest'
import { useRef, useState } from 'react'
import i18n from '../i18n'
import { buildTheme } from '../mantine'
import { CostFact } from '../measurement'
import { ClientSetupDialog } from './ClientSetupDialog'
import { ProtectionRequestPreview } from './ProtectionRequestPreview'
import { ModelFacts } from './ModelFacts'
import { ProbeForm } from './ProbeForm'
import OperationsPage from './OperationsPage'
import { json, mockApi, models, mount } from './referenceConsoleFixtures'

beforeEach(async () => {
  localStorage.clear()
  await i18n.changeLanguage('en')
})
function SensitiveClient() {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button onClick={() => setOpen(true)}>Connect test key</button>
      {open && (
        <ClientSetupDialog
          projectId="p1"
          keyId="test-key"
          keyName="Test key"
          token="test-only-once"
          onClose={() => setOpen(false)}
        />
      )}
    </>
  )
}
it.each(['escape', 'button'])(
  'client %s close removes sensitive DOM immediately and returns focus to connected keyboard opener',
  async (method) => {
    mount(
      <SensitiveClient />,
      '/keys',
      mockApi((path) =>
        path.includes('/client-models')
          ? json({ models: [{ id: 'gpt-4', metadata: { reasoning_levels: [] } }] })
          : undefined,
      ),
    )
    const trigger = await screen.findByRole('button', { name: 'Connect test key' })
    trigger.focus()
    await userEvent.keyboard('[Enter]')
    await screen.findByRole('dialog')
    expect(screen.getByText('test-only-once')).toBeInTheDocument()
    if (method === 'escape') await userEvent.keyboard('[Escape]')
    else await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Close' }))
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.queryByText('test-only-once')).not.toBeInTheDocument()
    await waitFor(() => expect(trigger).toHaveFocus())
  },
)
it.each(['text', 'request'])(
  'privacy %s keyboard close disposes sample/results and restores the actual opener',
  async (mode) => {
    mount(<ProtectionRequestPreview />, '/prompts', mockApi())
    const trigger = await screen.findByRole('button', { name: 'Preview rules' })
    trigger.focus()
    await userEvent.keyboard('[Enter]')
    await screen.findByRole('dialog')
    if (mode === 'request') await userEvent.click(screen.getByRole('tab', { name: 'Full request' }))
    const sample = screen.getByRole('textbox', { name: mode === 'request' ? 'JSON request' : 'Sample text' })
    fireEvent.change(sample, {
      target: { value: mode === 'request' ? '{"input":"private keyboard sample"}' : 'private keyboard sample' },
    })
    sample.focus()
    await userEvent.keyboard('[Escape]')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.queryByDisplayValue(/private keyboard sample/)).not.toBeInTheDocument()
    await waitFor(() => expect(trigger).toHaveFocus())
  },
)
it('primary monetary/pricing/settlement facts use the actual 14px house scale, preserving real zero and unavailable values', () => {
  expect(buildTheme('default', 'bronze', 'light').fontSizes?.md).toBe('14px')
  render(
    <CostFact row={{ pricing_status: 'incomplete_usage', settlement_kind: 'conservative', total_cost_micros: 25 }} />,
  )
  for (const text of ['—', 'Cost unavailable', 'Conservative estimate', 'Settled ledger amount: $0.000025']) {
    expect(screen.getByText(text)).toHaveAttribute('data-size', 'md')
  }
})
it('375px usage cards expose paid zero/free/missing/incomplete/legacy and conservative ledger facts', async () => {
  const rows = [
    { id: 'paid-zero', pricing_status: 'priced', cost_micros: 0, settlement_kind: 'reported' },
    { id: 'free-zero', pricing_status: 'explicit_free', cost_micros: 0, settlement_kind: 'reported' },
    { id: 'missing', pricing_status: 'missing_price', cost_micros: 0, settlement_kind: 'unreported' },
    { id: 'incomplete', pricing_status: 'incomplete_usage', cost_micros: 25, settlement_kind: 'conservative' },
    { id: 'historical', pricing_status: 'legacy', cost_micros: 99, settlement_kind: 'reported' },
  ].map((row) => ({ ...row, execution_id: row.id, input_tokens: 0, output_tokens: 0, usage_measurement: {} }))
  mount(
    <OperationsPage />,
    '/operations?tab=usage',
    mockApi((path) => (path.includes('/operations/usage') ? json({ data: rows, total: rows.length }) : undefined)),
  )
  await screen.findByRole('region', { name: 'Usage' })
  const mobile = within(document.querySelector('.mobile-resource-list')!)
  expect(mobile.getAllByText('$0.000000')).toHaveLength(2)
  expect(mobile.getByText('Missing price')).toBeInTheDocument()
  expect(mobile.getByText('Cost unavailable')).toBeInTheDocument()
  expect(mobile.getByText('Conservative estimate')).toBeInTheDocument()
  expect(mobile.getByText('Settled ledger amount: $0.000025')).toBeInTheDocument()
  expect(mobile.getByText('Historical amount')).toBeInTheDocument()
  expect(mobile.getByText('$0.000099')).toBeInTheDocument()
})
it.each(['text', 'request'])(
  'long %s preview results are named keyboard-focusable regions and decisions use readable theme contrast',
  async (mode) => {
    const result = {
      decision: 'redact',
      redacted_body: { input: 'retained'.repeat(4000) },
      findings: [],
      suppressed_findings: [],
      truncated: false,
    }
    const fetchMock = mockApi((path) =>
      path.endsWith('/protection-request-preview')
        ? json(result)
        : path.endsWith('/protection-preview')
          ? json({
              rules: [
                {
                  id: 'text-rule',
                  name: 'Test rule',
                  description: '',
                  action: 'redact',
                  enabled: true,
                  state: 'active',
                  matched: true,
                  result: 'retained'.repeat(4000),
                },
              ],
            })
          : undefined,
    )
    mount(<ProtectionRequestPreview />, '/prompts', fetchMock)
    await userEvent.click(await screen.findByRole('button', { name: 'Preview rules' }))
    if (mode === 'request') await userEvent.click(screen.getByRole('tab', { name: 'Full request' }))
    fireEvent.change(screen.getByRole('textbox', { name: mode === 'request' ? 'JSON request' : 'Sample text' }), {
      target: { value: mode === 'request' ? '{"input":"sample"}' : 'sample' },
    })
    await userEvent.click(screen.getByRole('button', { name: 'Run preview' }))
    const region = await screen.findByRole('region', { name: mode === 'request' ? 'Resulting request' : 'Result · Test rule (text-rule)' })
    expect(region).toHaveAttribute('tabindex', '0')
    region.focus()
    expect(region).toHaveFocus()
    if (mode === 'request') {
      const badge = screen.getByText('Redact').closest('.mantine-Badge-root')
      expect(badge).toBeVisible()
      expect(Number.parseFloat(getComputedStyle(badge!).fontSize)).toBeGreaterThanOrEqual(14)
    }
  },
)
it('model facts consume the bounded actual card snapshot including false, empty and known limits', async () => {
  const card = {
    capabilities: { tools: true, vision: false, reasoning: null },
    limits: { context: 1000000, output: 384000 },
    modalities: { input: ['text', 'image'], output: [] },
    reasoning_levels: [],
  }
  render(
    <ModelFacts
      row={{ id: 'real-catalog',
          public_name: 'Real catalog', catalog_metadata: { catalog_version: 'builtin', logo_key: 'openai', card } }}
    />,
  )
  await userEvent.click(screen.getByRole('button', { name: 'Model facts Real catalog' }))
  const dialog = within(screen.getByRole('dialog'))
  const facts = (key: string) => dialog.getByText(i18n.t(key)).parentElement!
  expect(facts('modelFact_tools')).toHaveTextContent('Supported')
  expect(facts('modelFact_vision')).toHaveTextContent('Unsupported')
  expect(facts('modelFact_context')).toHaveTextContent('1,000,000')
  expect(facts('modelFact_output')).toHaveTextContent('384,000')
  expect(facts('modelFact_outputModalities')).toHaveTextContent('None')
  expect(facts('modelFact_reasoning')).toHaveTextContent('Unknown')
})
it('intrinsic streaming=false in the real card envelope gates the actual probe picker submission', async () => {
  const fetchMock = mockApi((path) =>
    path.includes('/operations/models')
      ? json({
          data: [
            {
              ...models[0],
              catalog_metadata: { catalog_version: 'builtin', card: { capabilities: { streaming: false } } },
            },
          ],
          total: 1,
        })
      : undefined,
  )
  mount(<ProbeForm />, '/channels?tab=probes', fetchMock)
  await waitFor(() => expect(screen.getByRole('button', { name: 'Run probe' })).toBeEnabled())
  expect(screen.getByRole('switch', { name: 'Stream' })).toBeDisabled()
  await userEvent.click(screen.getByRole('button', { name: 'Run probe' }))
  await waitFor(() =>
    expect(
      fetchMock.mock.calls.some(
        ([, init]) => init?.method === 'POST' && JSON.parse(String(init.body)).stream === false,
      ),
    ).toBe(true),
  )
})
it.each([
  null,
  { card: [] },
  {
    card: {
      capabilities: { tools: 'true', vision: false },
      limits: { context: 0, output: 9007199254740992 },
      modalities: { input: ['text', 7], output: ['x'.repeat(33)] },
      reasoning_levels: Array(17).fill('low'),
    },
  },
])('malformed catalog envelopes and field bounds stay unknown without affirmative guesses', async (metadata) => {
  render(<ModelFacts row={{ id: 'unknown-catalog', public_name: 'Unknown catalog', catalog_metadata: metadata }} />)
  await userEvent.click(screen.getByRole('button', { name: 'Model facts Unknown catalog' }))
  const dialog = within(screen.getByRole('dialog'))
  for (const key of [
    'modelFact_tools',
    'modelFact_context',
    'modelFact_output',
    'modelFact_inputModalities',
    'modelFact_outputModalities',
    'modelFact_reasoningLevels',
  ])
    expect(dialog.getByText(i18n.t(key)).parentElement).toHaveTextContent('Unknown')
  if (metadata && 'card' in metadata && !Array.isArray(metadata.card))
    expect(dialog.getByText(i18n.t('modelFact_vision')).parentElement).toHaveTextContent('Unsupported')
})
function TokenHandoff() {
  const opener = useRef<HTMLButtonElement>(null)
  const temporary = useRef<HTMLButtonElement>(null)
  const scope = useRef(0)
  const [open, setOpen] = useState(false)
  return (
    <>
      <button
        ref={opener}
        onClick={() => {
          temporary.current?.focus()
          setOpen(true)
        }}
      >
        Create key
      </button>
      {!open && <button ref={temporary}>Submit transient form</button>}
      {open && (
        <ClientSetupDialog
          projectId="p1"
          keyId="new-key"
          keyName="New key"
          opener={opener.current}
          focusScope={() => `p1:${scope.current}`}
          token="test-only-handoff"
          onClose={() => {
            scope.current++
            setOpen(false)
          }}
        />
      )}
    </>
  )
}
it('one-time client handoff restores the connected initiating control after its intermediate form disappears', async () => {
  mount(
    <TokenHandoff />,
    '/keys',
    mockApi((path) => (path.includes('/client-models') ? json({ models: [{ id: 'gpt-4' }] }) : undefined)),
  )
  const trigger = await screen.findByRole('button', { name: 'Create key' })
  trigger.focus()
  await userEvent.keyboard('[Enter]')
  expect(await screen.findByText('test-only-handoff')).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Submit transient form' })).not.toBeInTheDocument()
  await userEvent.keyboard('[Escape]')
  expect(screen.queryByText('test-only-handoff')).not.toBeInTheDocument()
  await waitFor(() => expect(trigger).toHaveFocus())
})
it('project disposal keeps focus on the current project action and removes preview state without restoring the old context', async () => {
  mount(<ProtectionRequestPreview />, '/prompts', mockApi())
  await userEvent.click(await screen.findByRole('button', { name: 'Preview rules' }))
  fireEvent.change(screen.getByRole('textbox', { name: 'Sample text' }), {
    target: { value: 'discard-on-scope-change' },
  })
  const switcher = screen.getByRole('button', { name: 'Switch project' })
  switcher.focus()
  fireEvent.click(switcher)
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  expect(switcher).toHaveFocus()
  expect(screen.queryByDisplayValue('discard-on-scope-change')).not.toBeInTheDocument()
})
