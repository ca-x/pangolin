import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState, type FormEvent } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import {
  AllowedModelsEditor,
  AutoDisableEditor,
  ChannelSettingsEditor,
  EndpointMappingsEditor,
  ExclusionsEditor,
  ParameterOverridesEditor,
  ProfileMappingsEditor,
  RetryStatusesEditor,
  RoutingPolicyEditor,
  type EditorProps,
} from './documentEditors'

/**
 * Every editor submits one serialized document through a hidden textarea, so a
 * plain form harness is the whole contract: interact with the structured
 * controls, submit, and read back what the form would have sent.
 */
function Harness({ children, onDocument }: { children: (props: EditorProps) => React.ReactNode; onDocument: (value: unknown) => void }) {
  const [valid, setValid] = useState(true)
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    onDocument(JSON.parse(String(data.get('document') ?? '')))
  }
  return (
    <form onSubmit={submit}>
      {children({ name: 'document', label: 'Document', value: undefined, setValid })}
      <button type="submit" disabled={!valid}>Save</button>
    </form>
  )
}

const submitDocument = async (onDocument: (value: unknown) => void, ui: (props: EditorProps) => React.ReactNode) => {
  const user = userEvent.setup()
  render(<Harness onDocument={onDocument} children={ui} />)
  await user.click(screen.getByRole('button', { name: 'Save' }))
}

describe('document editors', () => {
  beforeEach(() => {
    vi.unstubAllGlobals()
    void i18n.changeLanguage('en')
  })

  it('serializes structured channel settings and preserves unknown keys', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <ChannelSettingsEditor name={name} label={label} value={{ version: 1, logo_key: 'lobehub:OpenAI' }} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.type(screen.getByRole('combobox', { name: /Channel tags/ }), 'edge,fast')
    await user.click(screen.getByRole('switch', { name: /Pass client User-Agent/ }))
    await user.click(screen.getByRole('button', { name: 'Limits and queueing' }))
    await user.type(await screen.findByRole('textbox', { name: 'RPM' }), '120')
    await user.click(screen.getByRole('button', { name: 'Circuit breaker' }))
    await user.click(await screen.findByRole('switch', { name: /Enable circuit breaker/ }))
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith({
      version: 1,
      logo_key: 'lobehub:OpenAI',
      tags: ['edge', 'fast'],
      pass_user_agent: true,
      limits: { rpm: 120 },
      circuit: { enabled: true, failures: 5, window_ms: 60000, recovery_ms: 30000 },
    })
  }, 15_000)

  it('keeps an enabled circuit breaker from being saved with a zero window', async () => {
    const user = userEvent.setup()
    render(
      <Harness onDocument={vi.fn()}>
        {({ name, label, value, setValid }) => (
          <ChannelSettingsEditor name={name} label={label} value={{ version: 1, circuit: { enabled: true, failures: 0, window_ms: 0, recovery_ms: 0 } }} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.click(screen.getByRole('button', { name: 'Circuit breaker' }))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent(/at least 1/))
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled()
  })

  it('edits endpoint mappings as rows and refuses an invalid upstream path', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <EndpointMappingsEditor name={name} label={label} value={{ version: 1, paths: { '/v1/messages': '/anthropic/v1/messages' } }} setValid={setValid} />
        )}
      </Harness>,
    )
    const path = screen.getByRole('textbox', { name: 'Upstream path 1' })
    await user.clear(path)
    await user.type(path, 'anthropic/v1/messages')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled())
    await user.clear(path)
    await user.type(path, '/anthropic/v1/messages')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith({ version: 1, paths: { '/v1/messages': '/anthropic/v1/messages' } })
  }, 15_000)

  it('keeps numbers and booleans typed in parameter overrides', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <ParameterOverridesEditor name={name} label={label} value={{ version: 1 }} setValid={setValid} />
        )}
      </Harness>,
    )
    const select = screen.getByRole('combobox', { name: 'Add parameter override' })
    await user.click(select)
    await user.click(await screen.findByRole('option', { name: 'temperature' }))
    await user.type(await screen.findByRole('textbox', { name: 'Value 1' }), '0.7')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith({ version: 1, temperature: 0.7 })
  }, 15_000)

  it('toggles retry statuses and applies a preset', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <RetryStatusesEditor name={name} label={label} value={{ version: 1, statuses: [408] }} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.click(screen.getByRole('button', { name: '429' }))
    await user.click(screen.getByRole('button', { name: 'Conservative' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith({ version: 1, statuses: [408, 429, 502, 503, 504] })
  })

  it('serializes the auto-disable policy and refuses an out-of-range duration', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <AutoDisableEditor name={name} label={label} value={{ version: 1, enabled: false, threshold: 3, duration_secs: 300 }} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.click(screen.getByRole('switch', { name: /Enable auto-disable/ }))
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith({ version: 1, enabled: true, threshold: 3, duration_secs: 300 })

    const duration = screen.getByRole('textbox', { name: 'Disable duration (s)' })
    await user.clear(duration)
    await user.type(duration, '999999')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled())
  }, 15_000)

  it('serializes the routing policy document', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <RoutingPolicyEditor name={name} label={label} value={{ version: 1, strategy: 'failover', sticky: 'off', max_attempts: 3, allowed_tags: [], tag_mode: 'any', limits: {} }} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.click(screen.getByRole('combobox', { name: 'Load strategy' }))
    await user.click(await screen.findByRole('option', { name: /Least in-flight/ }))
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith(expect.objectContaining({ version: 1, strategy: 'least_inflight', max_attempts: 3 }))
  }, 15_000)

  it('blocks a routing policy with a zero attempt ceiling', async () => {
    render(
      <Harness onDocument={vi.fn()}>
        {({ name, label, value, setValid }) => (
          <RoutingPolicyEditor name={name} label={label} value={{ version: 1, max_attempts: 0 }} setValid={setValid} />
        )}
      </Harness>,
    )
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled())
  })

  it('edits profile model mappings and refuses a duplicate source', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <ProfileMappingsEditor name={name} label={label} value={[{ source_model: 'gpt-4o', target_model: 'upstream-4o', priority: 100 }]} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.click(screen.getByRole('button', { name: 'Add mapping' }))
    await user.type(screen.getByRole('textbox', { name: 'Original name 2' }), 'gpt-4o')
    await user.type(screen.getByRole('textbox', { name: 'Transformed name 2' }), 'other-4o')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled())
    await user.clear(screen.getByRole('textbox', { name: 'Original name 2' }))
    await user.type(screen.getByRole('textbox', { name: 'Original name 2' }), 'gpt-4o-mini')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith([
      { source_model: 'gpt-4o', target_model: 'upstream-4o', priority: 100 },
      { source_model: 'gpt-4o-mini', target_model: 'other-4o', priority: 100 },
    ])
  }, 15_000)

  it('adds an allowed-model pattern row', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <AllowedModelsEditor name={name} label={label} value={[]} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.click(screen.getByRole('button', { name: 'Add model' }))
    await user.type(screen.getByRole('textbox', { name: 'Model name or pattern 1' }), 'deepseek-.*')
    await user.click(screen.getByRole('combobox', { name: 'Match type 1' }))
    await user.click(await screen.findByRole('option', { name: 'Regex' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith([{ pattern: 'deepseek-.*', match_type: 'regex' }])
  }, 15_000)

  it('picks excluded channels from the page options', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <ExclusionsEditor
            name={name}
            label={label}
            value={{ version: 1, channel_ids: [], channel_tags: [], channel_name_patterns: [] }}
            setValid={setValid}
            channels={[{ value: 'c1', label: 'Primary' }, { value: 'c2', label: 'Backup' }]}
          />
        )}
      </Harness>,
    )
    await user.click(await screen.findByRole('combobox', { name: /Excluded channel IDs/ }))
    await user.click(await screen.findByRole('option', { name: 'Backup' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith({ version: 1, channel_ids: ['c2'], channel_tags: [], channel_name_patterns: [] })
  }, 15_000)

  it('falls back to the raw JSON view and blocks unparseable documents', async () => {
    const user = userEvent.setup()
    const captured = vi.fn()
    render(
      <Harness onDocument={captured}>
        {({ name, label, value, setValid }) => (
          <RetryStatusesEditor name={name} label={label} value={{ version: 1, statuses: [] }} setValid={setValid} />
        )}
      </Harness>,
    )
    await user.click(screen.getByRole('button', { name: 'JSON' }))
    const raw = screen.getByRole('textbox', { name: 'Document' })
    fireEvent.change(raw, { target: { value: '{' } })
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled())
    fireEvent.change(raw, { target: { value: '{"version":1,"statuses":[500]}' } })
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(captured).toHaveBeenCalledWith({ version: 1, statuses: [500] })
  })

  it('localizes every editor label in both languages', () => {
    const keys = [
      'editorStructured', 'editorRawJson', 'channelSettingsHint', 'channelTags', 'passUserAgent',
      'sectionLimits', 'sectionCircuit', 'sectionQuota', 'sectionAdvanced', 'sectionChannelTags',
      'limitConcurrent', 'circuitEnabled', 'quotaPath', 'endpointMappingsHint', 'gatewayEndpoint',
      'parameterOverridesHint', 'retryStatusesHint', 'autoDisableHint', 'autoDisableThreshold',
      'routingPolicyHint', 'loadBalanceStrategy', 'stickyMode', 'maxAttempts', 'allowedTags', 'tagMode',
      'profileMappingsHint', 'allowedModelsHint', 'exclusionsHint', 'multiChannelCreateHint',
      'channelPriorityHint', 'importSearchPlaceholder', 'channelCloneName',
    ]
    for (const key of keys) {
      expect(i18n.exists(key, { lng: 'en' }), key).toBe(true)
      expect(i18n.exists(key, { lng: 'zh-CN' }), key).toBe(true)
    }
  })
})
