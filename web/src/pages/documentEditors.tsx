import { ActionIcon, Alert, Button, Collapse, Group, NumberInput, Paper, Select, SimpleGrid, Stack, Switch, TagsInput, Text, TextInput, Textarea, Tooltip, UnstyledButton } from '@mantine/core'
import { ChevronRight, Plus, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * Structured editors for the versioned JSON documents the record system stores
 * (channel settings and policies, key-profile routing, association exclusions).
 * One component per document keeps the wire contract unchanged: the hidden
 * textarea carries the serialized document into the form's FormData path, so
 * `readForm` and the backend see exactly what a hand-written JSON box would
 * have sent. Structured controls edit the common cases; the raw view stays one
 * click away for every rule this file does not model, and unknown keys in an
 * existing document are preserved rather than silently dropped.
 */
export type EditorProps = { name: string; label: string; value: unknown; setValid: (valid: boolean) => void }

/** The document editors deal in: a versioned object or a plain row array. */
type Doc = Record<string, unknown> | unknown[]

const asObject = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
const asArray = (value: unknown): unknown[] => (Array.isArray(value) ? value : [])
const objectList = (value: unknown): Record<string, unknown>[] =>
  asArray(value).filter((row): row is Record<string, unknown> => Boolean(row) && typeof row === 'object' && !Array.isArray(row))
const stringList = (value: unknown): string[] => asArray(value).filter((row): row is string => typeof row === 'string' && row.length > 0)
const nested = (doc: unknown, key: string): Record<string, unknown> => asObject(asObject(doc)[key])
const numberOr = (value: unknown, fallback: number): number => typeof value === 'number' && Number.isFinite(value) ? value : fallback
const numberOrNull = (value: unknown): number | '' => typeof value === 'number' && Number.isFinite(value) ? value : ''
const optionalNumber = (value: number | ''): number | null => value === '' || value === null ? null : Number(value)

/**
 * Shared document state. The structured view is authoritative while it is
 * active; the raw view takes over the moment it is edited, and an unparseable
 * raw document blocks the submit through `setValid` exactly like an invalid
 * structured value would.
 */
function useDocEditor<T extends Doc>(initial: T, validate: (doc: T) => string | undefined) {
  const serializedInitial = useMemo(() => JSON.stringify(initial), [initial])
  const [doc, setDoc] = useState<T>(() => JSON.parse(serializedInitial) as T)
  const [text, setText] = useState(() => JSON.stringify(JSON.parse(serializedInitial) as T, null, 2))
  const [mode, setMode] = useState<'structured' | 'raw'>('structured')
  const [unparseable, setUnparseable] = useState(false)
  useEffect(() => {
    const next = JSON.parse(serializedInitial) as T
    setDoc(next)
    setText(JSON.stringify(next, null, 2))
    setMode('structured')
    setUnparseable(false)
  }, [serializedInitial])
  const change = (next: T) => { setDoc(next); setText(JSON.stringify(next, null, 2)); setUnparseable(false) }
  const changeText = (next: string) => {
    setText(next)
    try { setDoc(JSON.parse(next) as T); setUnparseable(false) } catch { setUnparseable(true) }
  }
  const error = unparseable ? 'invalidJson' : validate(doc)
  return { doc, rawDoc: doc, text, mode, setMode, change, changeText, error, valid: error === undefined, initialKey: serializedInitial }
}

/** One collapsible group inside a structured view. Sections with content start
 * open; an empty group stays out of the way — progressive disclosure without
 * hiding configured state. */
function EditorSection({ title, defaultOpen, children }: { title: string; defaultOpen: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(defaultOpen)
  return (
    <Paper withBorder radius="md" style={{ overflow: 'hidden' }}>
      <UnstyledButton onClick={() => setOpen((value) => !value)} aria-expanded={open} w="100%" px="sm" py="xs">
        <Group justify="space-between" gap="xs" wrap="nowrap">
          <Text size="sm" fw={600}>{title}</Text>
          <ChevronRight size={15} aria-hidden="true" style={{ transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 150ms var(--ease-out, ease-out)' }} />
        </Group>
      </UnstyledButton>
      <Collapse expanded={open} keepMounted>
        <div inert={!open}>
          <Stack gap="sm" px="sm" pb="sm">{children}</Stack>
        </div>
      </Collapse>
    </Paper>
  )
}

function DocEditorShell({ label, hint, name, text, mode, onModeChange, onTextChange, error, children }: {
  label: string
  hint?: string
  name: string
  text: string
  mode: 'structured' | 'raw'
  onModeChange: (mode: 'structured' | 'raw') => void
  onTextChange: (text: string) => void
  error?: string
  children: ReactNode
}) {
  const { t } = useTranslation()
  return (
    <Stack gap="sm">
      <Group justify="space-between" align="flex-start" wrap="wrap" gap="sm">
        <Stack gap={2} style={{ minWidth: 0 }}>
          <Text fw={600} size="sm">{label}</Text>
          {hint && <Text size="xs" c="dimmed">{hint}</Text>}
        </Stack>
        <Group gap={4}>
          <Button variant={mode === 'structured' ? 'filled' : 'subtle'} size="compact-sm" onClick={() => onModeChange('structured')}>{t('editorStructured')}</Button>
          <Button variant={mode === 'raw' ? 'filled' : 'subtle'} size="compact-sm" onClick={() => onModeChange('raw')}>{t('editorRawJson')}</Button>
        </Group>
      </Group>
      {mode === 'raw'
        ? <Textarea aria-label={label} value={text} onChange={(event) => onTextChange(event.currentTarget.value)} rows={9} styles={{ input: { fontFamily: 'var(--font-mono)' } }} />
        : children}
      {error && <Alert color="red" variant="light" role="alert">{t(error)}</Alert>}
      {/* The serialized document rides in a hidden textarea: the field keeps the
          FormData contract every other JSON field already uses. */}
      <textarea name={name} value={text} readOnly hidden />
    </Stack>
  )
}

/** Labeled row actions stay reachable from the keyboard and keep the 40px
 * in-table touch height the design system reserves for icon buttons. */
function RemoveRowButton({ label, index, onClick }: { label: string; index: number; onClick: () => void }) {
  return (
    <Tooltip label={label}>
      <ActionIcon size={40} variant="subtle" color="red" aria-label={`${label} ${index + 1}`} onClick={onClick}>
        <Trash2 size={16} />
      </ActionIcon>
    </Tooltip>
  )
}

// ---------------------------------------------------------------------------
// Channel settings document: tags, limits, circuit, quota path, user agent.
// The backend reads these keys in `providers.settings_json`.
// ---------------------------------------------------------------------------

export function ChannelSettingsEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const editor = useDocEditor(asObject(value).version === undefined ? { version: 1, ...asObject(value) } : asObject(value), (doc) => {
    const circuit = nested(doc, 'circuit')
    if (circuit.enabled === true) {
      for (const key of ['failures', 'window_ms', 'recovery_ms']) {
        if (numberOr(circuit[key], 0) < 1) return 'circuitInvalid'
      }
    }
    const limits = nested(doc, 'limits')
    for (const key of ['rpm', 'tpm', 'concurrent', 'queue', 'queue_timeout_ms']) {
      if (limits[key] != null && (typeof limits[key] !== 'number' || limits[key] < 0)) return 'nonNegativeNumber'
    }
    const path = nested(doc, 'quota').path
    if (typeof path === 'string' && path && !path.startsWith('/')) return 'invalidHttpPath'
    return undefined
  })
  const { doc } = editor
  const limits = nested(doc, 'limits')
  const circuit = nested(doc, 'circuit')
  const quota = nested(doc, 'quota')
  const limitsConfigured = Object.keys(limits).length > 0
  const circuitConfigured = circuit.enabled === true || (Object.keys(circuit).length > 0 && numberOr(circuit.failures, 5) !== 5)
  const quotaConfigured = typeof quota.path === 'string' && quota.path.length > 0
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('channelSettingsHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      <TagsInput
        label={t('channelTags')}
        description={t('channelTagsHint')}
        value={stringList(doc.tags)}
        onChange={(tags) => editor.change({ ...doc, tags: tags.map((tag) => tag.trim()).filter(Boolean) })}
      />
      <Switch
        label={t('passUserAgent')}
        description={t('passUserAgentHint')}
        checked={doc.pass_user_agent === true}
        onChange={(event) => editor.change({ ...doc, pass_user_agent: event.currentTarget.checked })}
      />
      <EditorSection title={t('sectionLimits')} defaultOpen={limitsConfigured}>
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <NumberInput label="RPM" min={0} hideControls value={numberOrNull(limits.rpm)} onChange={(next) => editor.change({ ...doc, limits: { ...limits, rpm: optionalNumber(next as number | '') } })} />
          <NumberInput label="TPM" min={0} hideControls value={numberOrNull(limits.tpm)} onChange={(next) => editor.change({ ...doc, limits: { ...limits, tpm: optionalNumber(next as number | '') } })} />
          <NumberInput label={t('limitConcurrent')} min={0} hideControls value={numberOrNull(limits.concurrent)} onChange={(next) => editor.change({ ...doc, limits: { ...limits, concurrent: optionalNumber(next as number | '') } })} />
          <NumberInput label={t('limitQueue')} min={0} hideControls value={numberOrNull(limits.queue)} onChange={(next) => editor.change({ ...doc, limits: { ...limits, queue: optionalNumber(next as number | '') ?? 0 } })} />
        </SimpleGrid>
        <NumberInput label={t('limitQueueTimeout')} min={0} hideControls value={numberOrNull(limits.queue_timeout_ms)} onChange={(next) => editor.change({ ...doc, limits: { ...limits, queue_timeout_ms: optionalNumber(next as number | '') ?? 0 } })} />
      </EditorSection>
      <EditorSection title={t('sectionCircuit')} defaultOpen={circuitConfigured}>
        <Switch
          label={t('circuitEnabled')}
          description={t('circuitEnabledHint')}
          checked={circuit.enabled === true}
          onChange={(event) => editor.change({ ...doc, circuit: event.currentTarget.checked
            ? { enabled: true, failures: numberOr(circuit.failures, 5), window_ms: numberOr(circuit.window_ms, 60000), recovery_ms: numberOr(circuit.recovery_ms, 30000) }
            : { ...circuit, enabled: false } })}
        />
        <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
          <NumberInput label={t('circuitFailures')} min={1} hideControls value={numberOr(circuit.failures, 5)} onChange={(next) => editor.change({ ...doc, circuit: { ...circuit, failures: numberOr(next as number, 5) } })} />
          <NumberInput label={t('circuitWindow')} min={1} hideControls value={numberOr(circuit.window_ms, 60000)} onChange={(next) => editor.change({ ...doc, circuit: { ...circuit, window_ms: numberOr(next as number, 60000) } })} />
          <NumberInput label={t('circuitRecovery')} min={1} hideControls value={numberOr(circuit.recovery_ms, 30000)} onChange={(next) => editor.change({ ...doc, circuit: { ...circuit, recovery_ms: numberOr(next as number, 30000) } })} />
        </SimpleGrid>
      </EditorSection>
      <EditorSection title={t('sectionQuota')} defaultOpen={quotaConfigured}>
        <TextInput
          label={t('quotaPath')}
          description={t('quotaPathHint')}
          placeholder="/api/v1/key"
          value={typeof quota.path === 'string' ? quota.path : ''}
          onChange={(event) => editor.change({ ...doc, quota: { ...quota, path: event.currentTarget.value } })}
        />
      </EditorSection>
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Endpoint mappings document: { version, paths: { [gateway endpoint]: upstream path } }.
// ---------------------------------------------------------------------------

const KNOWN_ENDPOINTS = [
  '/v1/chat/completions', '/v1/completions', '/v1/responses', '/v1/responses/compact',
  '/v1/messages', '/v1/embeddings', '/v1/moderations', '/v1/alpha/search',
  '/v1/images/generations', '/v1/images/edits', '/v1/videos', '/v1/audio/speech',
  '/v1/audio/transcriptions', '/v1/audio/translations', '/v1/rerank',
  '/v1beta/models:generateContent', '/v1beta/models:streamGenerateContent',
  '/doubao/v3/contents/generations/tasks',
] as const

export function EndpointMappingsEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const editor = useDocEditor(asObject(value).version === undefined ? { version: 1, ...asObject(value) } : asObject(value), (doc) => {
    const paths = nested(doc, 'paths')
    const entries = Object.entries(paths)
    if (entries.some(([endpoint, path]) => !endpoint.startsWith('/'))) return 'invalidHttpPath'
    if (entries.some(([, path]) => {
      const target = String(path ?? '')
      return !target.startsWith('/') || target.startsWith('//') || target.includes('?') || target.includes('#')
    })) return 'invalidHttpPath'
    return undefined
  })
  const { doc } = editor
  const paths = nested(doc, 'paths')
  // Rows are a draft: an emptied field stays editable in place, and only
  // complete rows reach the wire document — clearing an input must not delete
  // the row out from under the cursor.
  const rowsOf = (document: Record<string, unknown>) => Object.entries(nested(document, 'paths')).map(([endpoint, path], index) => ({ id: index, endpoint, path: String(path ?? '') }))
  const [rows, setRows] = useState(() => rowsOf(doc))
  const [nextId, setNextId] = useState(rows.length)
  useEffect(() => { const fresh = rowsOf(editor.doc); setRows(fresh); setNextId((current) => Math.max(current, fresh.length)) }, [editor.initialKey])
  useEffect(() => { if (editor.mode === 'structured') setRows(rowsOf(editor.doc)) }, [editor.mode])
  const unused = KNOWN_ENDPOINTS.filter((endpoint) => !(endpoint in paths))
  const writeRow = (next: Array<{ id: number; endpoint: string; path: string }>) => {
    setRows(next)
    editor.change({ ...doc, paths: Object.fromEntries(next.filter((row) => row.endpoint.trim() && row.path.trim()).map((row) => [row.endpoint, row.path] as [string, string])) })
  }
  const updateRow = (id: number, patch: Partial<{ endpoint: string; path: string }>) =>
    writeRow(rows.map((row) => row.id === id ? { ...row, ...patch } : row))
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('endpointMappingsHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      {rows.length === 0 && <Text size="sm" c="dimmed">{t('noEndpointMappings')}</Text>}
      {rows.map((row, index) => (
        <Group key={row.id} gap="xs" align="flex-start" wrap="nowrap">
          <TextInput aria-label={`${t('gatewayEndpoint')} ${index + 1}`} className="mono-cell" value={row.endpoint} readOnly style={{ flex: '0 0 auto', width: '46%' }} />
          <TextInput aria-label={`${t('upstreamPath')} ${index + 1}`} className="mono-cell" placeholder="/upstream/path" value={row.path} error={row.path.startsWith('//') || row.path.includes('?') || row.path.includes('#') ? t('invalidHttpPath') : undefined} onChange={(event) => updateRow(row.id, { path: event.currentTarget.value })} style={{ flex: 1, minWidth: 0 }} />
          <RemoveRowButton label={t('removeMapping')} index={index} onClick={() => writeRow(rows.filter((item) => item.id !== row.id))} />
        </Group>
      ))}
      {unused.length > 0 && (
        <Select
          label={t('addEndpointMapping')}
          placeholder="/v1/chat/completions"
          data={unused.map((endpoint) => ({ value: endpoint, label: endpoint }))}
          value={null}
          onChange={(endpoint) => { if (endpoint) { writeRow([...rows, { id: nextId, endpoint, path: endpoint }]); setNextId((current) => current + 1) } }}
        />
      )}
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Parameter overrides document: { version, [parameter]: value } merged into the
// upstream request body.
// ---------------------------------------------------------------------------

const COMMON_OVERRIDE_PARAMS = ['temperature', 'top_p', 'max_tokens', 'max_completion_tokens', 'stop', 'reasoning_effort', 'frequency_penalty', 'presence_penalty'] as const

/** Numbers and booleans stay typed; anything else is sent as a string. */
const parseOverrideValue = (raw: string): unknown => {
  const trimmed = raw.trim()
  if (trimmed === 'true') return true
  if (trimmed === 'false') return false
  if (trimmed !== '' && Number.isFinite(Number(trimmed))) return Number(trimmed)
  return raw
}

export function ParameterOverridesEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const editor = useDocEditor(asObject(value).version === undefined ? { version: 1, ...asObject(value) } : asObject(value), (doc) => {
    const keys = Object.keys(doc).filter((key) => key !== 'version')
    if (keys.some((key) => !key.trim())) return 'invalidOverrideKey'
    return undefined
  })
  const { doc } = editor
  // Draft rows: an emptied key stays in the editor and simply drops out of the
  // wire document instead of deleting the row mid-edit.
  const rowsOf = (document: Record<string, unknown>) => Object.entries(document).filter(([key]) => key !== 'version').map(([key, item], index) => ({ id: index, key, raw: typeof item === 'string' ? item : String(item) }))
  const [entries, setEntries] = useState(() => rowsOf(doc))
  const [nextId, setNextId] = useState(entries.length)
  useEffect(() => { const fresh = rowsOf(editor.doc); setEntries(fresh); setNextId((current) => Math.max(current, fresh.length)) }, [editor.initialKey])
  useEffect(() => { if (editor.mode === 'structured') setEntries(rowsOf(editor.doc)) }, [editor.mode])
  const unused = COMMON_OVERRIDE_PARAMS.filter((key) => !(key in doc))
  const write = (next: Array<{ id: number; key: string; raw: string }>) => {
    setEntries(next)
    editor.change({ version: 1, ...Object.fromEntries(next.filter((row) => row.key.trim()).map((row) => [row.key, parseOverrideValue(row.raw)])) })
  }
  const update = (id: number, patch: Partial<{ key: string; raw: string }>) =>
    write(entries.map((row) => row.id === id ? { ...row, ...patch } : row))
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('parameterOverridesHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      {entries.length === 0 && <Text size="sm" c="dimmed">{t('noParameterOverrides')}</Text>}
      {entries.map((row, index) => (
        <Group key={row.id} gap="xs" align="flex-start" wrap="nowrap">
          <TextInput aria-label={`${t('parameterName')} ${index + 1}`} className="mono-cell" value={row.key} onChange={(event) => update(row.id, { key: event.currentTarget.value })} style={{ flex: '0 0 auto', width: '40%' }} />
          <TextInput aria-label={`${t('parameterValue')} ${index + 1}`} className="mono-cell" placeholder="0.7 / true / text" value={row.raw} onChange={(event) => update(row.id, { raw: event.currentTarget.value })} style={{ flex: 1, minWidth: 0 }} />
          <RemoveRowButton label={t('removeMapping')} index={index} onClick={() => write(entries.filter((item) => item.id !== row.id))} />
        </Group>
      ))}
      {unused.length > 0 && (
        <Select
          label={t('addParameterOverride')}
          data={[...unused.map((key) => ({ value: key, label: key })), { value: '__custom__', label: t('customParameter') }]}
          value={null}
          onChange={(key) => { if (key) { write([...entries, { id: nextId, key: key === '__custom__' ? '' : key, raw: '' }]); setNextId((current) => current + 1) } }}
        />
      )}
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Retry statuses document: { version, statuses: [http status code] }.
// ---------------------------------------------------------------------------

const COMMON_RETRY_STATUSES = [408, 409, 429, 500, 502, 503, 504] as const
const RETRY_PRESETS: Array<{ key: string; statuses: number[] }> = [
  { key: 'retryPresetDefault', statuses: [408, 409, 429, 500, 502, 503, 504] },
  { key: 'retryPresetConservative', statuses: [408, 429, 502, 503, 504] },
  { key: 'retryPresetAggressive', statuses: [408, 409, 425, 429, 500, 502, 503, 504] },
]

export function RetryStatusesEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const editor = useDocEditor(asObject(value).version === undefined ? { version: 1, ...asObject(value) } : asObject(value), (doc) => {
    const statuses = asArray(doc.statuses)
    if (statuses.some((status) => !Number.isInteger(status) || (status as number) < 100 || (status as number) > 599)) return 'invalidStatusCode'
    if (new Set(statuses).size !== statuses.length) return 'invalidStatusCode'
    return undefined
  })
  const { doc } = editor
  const statuses = asArray(doc.statuses).map(Number)
  const known = COMMON_RETRY_STATUSES as readonly number[]
  const write = (next: number[]) => editor.change({ ...doc, statuses: [...new Set(next)].sort((a, b) => a - b) })
  const toggle = (status: number, on: boolean) => write(on ? [...statuses, status] : statuses.filter((item) => item !== status))
  const [custom, setCustom] = useState<number | ''>('')
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('retryStatusesHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      <Group gap="xs">
        {COMMON_RETRY_STATUSES.map((status) => {
          const on = statuses.includes(status)
          return (
            <Button
              key={status}
              size="compact-sm"
              variant={on ? 'filled' : 'default'}
              aria-pressed={on}
              onClick={() => toggle(status, !on)}
              className="mono-cell"
            >
              {status}
            </Button>
          )
        })}
      </Group>
      {statuses.filter((status) => !known.includes(status)).length > 0 && (
        <Group gap="xs">
          {statuses.filter((status) => !known.includes(status)).map((status) => (
            <Button key={status} size="compact-sm" variant="light" className="mono-cell" aria-label={`${t('removeMapping')} ${status}`} onClick={() => toggle(status, false)}>{status} ×</Button>
          ))}
        </Group>
      )}
      <Group gap="xs" align="flex-end" wrap="nowrap">
        <NumberInput label={t('customStatus')} min={100} max={599} hideControls value={custom} onChange={(next) => setCustom(next as number | '')} w={120} />
        <Button variant="default" size="sm" disabled={custom === '' || !Number.isInteger(custom)} onClick={() => { write([...statuses, Number(custom)]); setCustom('') }}>
          {t('addStatus')}
        </Button>
      </Group>
      <Group gap="xs">
        {RETRY_PRESETS.map((preset) => (
          <Button key={preset.key} variant="subtle" size="compact-sm" onClick={() => write(preset.statuses)}>{t(preset.key)}</Button>
        ))}
      </Group>
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Auto-disable policy document. See `operations::runtime::health_for` for the
// consuming semantics: statuses/pattern gate which failures count, action picks
// the channel or the single credential, recovery_cron overrides the duration.
// ---------------------------------------------------------------------------

export function AutoDisableEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const editor = useDocEditor(asObject(value).version === undefined ? { version: 1, ...asObject(value) } : asObject(value), (doc) => {
    const threshold = numberOr(doc.threshold, 3)
    if (threshold < 1) return 'autoDisableThresholdRange'
    const duration = numberOr(doc.duration_secs, 300)
    if (duration < 1 || duration > 86400) return 'autoDisableDurationRange'
    const pattern = doc.pattern
    if (typeof pattern === 'string' && pattern.length > 512) return 'invalidRegex'
    if (typeof pattern === 'string' && pattern) {
      try { new RegExp(pattern) } catch { return 'invalidRegex' }
    }
    const cron = doc.recovery_cron
    if (typeof cron === 'string' && cron) {
      const fields = cron.trim().split(/\s+/).length
      if (fields !== 5 && fields !== 6) return 'invalidCron'
    }
    return undefined
  })
  const { doc } = editor
  const statuses = asArray(doc.statuses).map(Number)
  const cronConfigured = typeof doc.recovery_cron === 'string' && Boolean(doc.recovery_cron)
  const patternConfigured = typeof doc.pattern === 'string' && Boolean(doc.pattern)
  const write = (patch: Record<string, unknown>) => editor.change({ ...doc, ...patch })
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('autoDisableHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      <Switch
        label={t('autoDisableEnabled')}
        description={t('autoDisableEnabledHint')}
        checked={doc.enabled === true}
        onChange={(event) => write({ enabled: event.currentTarget.checked })}
      />
      <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
        <NumberInput label={t('autoDisableThreshold')} min={1} hideControls value={numberOr(doc.threshold, 3)} onChange={(next) => write({ threshold: numberOr(next as number, 3) })} />
        <NumberInput label={t('autoDisableDuration')} min={1} max={86400} hideControls value={numberOr(doc.duration_secs, 300)} onChange={(next) => write({ duration_secs: numberOr(next as number, 300) })} />
      </SimpleGrid>
      <Select
        label={t('autoDisableAction')}
        data={[{ value: 'channel', label: t('autoDisableActionChannel') }, { value: 'credential', label: t('autoDisableActionCredential') }]}
        value={doc.action === 'credential' ? 'credential' : 'channel'}
        onChange={(action) => write({ action: action || 'channel' })}
      />
      <EditorSection title={t('sectionAdvanced')} defaultOpen={patternConfigured || cronConfigured || statuses.length > 0}>
        <TagsInput
          label={t('autoDisableStatuses')}
          description={t('autoDisableStatusesHint')}
          value={statuses.map(String)}
          onChange={(next) => write({ statuses: next.map(Number).filter((status) => Number.isInteger(status) && status >= 100 && status <= 599) })}
        />
        <TextInput
          label={t('autoDisablePattern')}
          description={t('autoDisablePatternHint')}
          className="mono-cell"
          value={typeof doc.pattern === 'string' ? doc.pattern : ''}
          onChange={(event) => write({ pattern: event.currentTarget.value })}
        />
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <TextInput
            label={t('recoveryCron')}
            description={t('recoveryCronHint')}
            className="mono-cell"
            placeholder="0 */6 * * *"
            value={typeof doc.recovery_cron === 'string' ? doc.recovery_cron : ''}
            onChange={(event) => write({ recovery_cron: event.currentTarget.value })}
          />
          <TextInput
            label={t('recoveryTimezone')}
            className="mono-cell"
            placeholder="UTC"
            value={typeof doc.timezone === 'string' ? doc.timezone : ''}
            onChange={(event) => write({ timezone: event.currentTarget.value })}
          />
        </SimpleGrid>
      </EditorSection>
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Key-profile routing policy. The backend parses this with
// `deny_unknown_fields`, so unknown keys stay untouched rather than stripped.
// ---------------------------------------------------------------------------

const ROUTING_STRATEGIES = [
  { value: 'failover', key: 'strategyFailover' },
  { value: 'round_robin', key: 'strategyRoundRobin' },
  { value: 'weighted', key: 'strategyWeighted' },
  { value: 'least_inflight', key: 'strategyLeastInflight' },
  { value: 'latency', key: 'strategyLatency' },
  { value: 'adaptive', key: 'strategyAdaptive' },
] as const

const STICKY_MODES = [
  { value: 'off', key: 'stickyOff' },
  { value: 'trace', key: 'stickyTrace' },
  { value: 'session', key: 'stickySession' },
] as const

export function RoutingPolicyEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const editor = useDocEditor(asObject(value).version === undefined ? { version: 1, ...asObject(value) } : asObject(value), (doc) => {
    const attempts = numberOr(doc.max_attempts, 3)
    if (attempts < 1 || attempts > 32) return 'maxAttemptsRange'
    return undefined
  })
  const { doc } = editor
  const limits = nested(doc, 'limits')
  const allowedTags = stringList(doc.allowed_tags)
  const tagConfigured = allowedTags.length > 0
  const limitsConfigured = Object.keys(limits).length > 0
  const write = (patch: Record<string, unknown>) => editor.change({ ...doc, ...patch })
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('routingPolicyHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
        <Select
          label={t('loadBalanceStrategy')}
          data={ROUTING_STRATEGIES.map((strategy) => ({ value: strategy.value, label: t(strategy.key) }))}
          value={typeof doc.strategy === 'string' ? doc.strategy : 'failover'}
          onChange={(strategy) => write({ strategy: strategy || 'failover' })}
        />
        <Select
          label={t('stickyMode')}
          data={STICKY_MODES.map((mode) => ({ value: mode.value, label: t(mode.key) }))}
          value={typeof doc.sticky === 'string' ? doc.sticky : 'off'}
          onChange={(sticky) => write({ sticky: sticky || 'off' })}
        />
        <NumberInput label={t('maxAttempts')} min={1} max={32} hideControls value={numberOr(doc.max_attempts, 3)} onChange={(next) => write({ max_attempts: numberOr(next as number, 3) })} />
      </SimpleGrid>
      <EditorSection title={t('sectionChannelTags')} defaultOpen={tagConfigured}>
        <TagsInput
          label={t('allowedTags')}
          description={t('allowedTagsHint')}
          value={allowedTags}
          onChange={(tags) => write({ allowed_tags: tags.map((tag) => tag.trim()).filter(Boolean) })}
        />
        <Select
          label={t('tagMode')}
          data={[{ value: 'any', label: t('tagModeAny') }, { value: 'all', label: t('tagModeAll') }, { value: 'none', label: t('tagModeNone') }]}
          value={doc.tag_mode === 'all' || doc.tag_mode === 'none' ? String(doc.tag_mode) : 'any'}
          onChange={(mode) => write({ tag_mode: mode || 'any' })}
        />
      </EditorSection>
      <EditorSection title={t('sectionLimits')} defaultOpen={limitsConfigured}>
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <NumberInput label="RPM" min={0} hideControls value={numberOrNull(limits.rpm)} onChange={(next) => write({ limits: { ...limits, rpm: optionalNumber(next as number | '') } })} />
          <NumberInput label="TPM" min={0} hideControls value={numberOrNull(limits.tpm)} onChange={(next) => write({ limits: { ...limits, tpm: optionalNumber(next as number | '') } })} />
          <NumberInput label={t('limitConcurrent')} min={0} hideControls value={numberOrNull(limits.concurrent)} onChange={(next) => write({ limits: { ...limits, concurrent: optionalNumber(next as number | '') } })} />
          <NumberInput label={t('limitQueue')} min={0} hideControls value={numberOrNull(limits.queue)} onChange={(next) => write({ limits: { ...limits, queue: optionalNumber(next as number | '') ?? 0 } })} />
        </SimpleGrid>
      </EditorSection>
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Key-profile model mappings: [{ source_model, target_model, priority }].
// A source may carry a `regex:` prefix for pattern mapping.
// ---------------------------------------------------------------------------

type MappingDraft = { id: number; source: string; target: string; priority: number | '' }

export function ProfileMappingsEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const rowsOf = (doc: Doc): MappingDraft[] => objectList(doc).map((row, index) => ({
    id: index,
    source: typeof row.source_model === 'string' ? row.source_model : '',
    target: typeof row.target_model === 'string' ? row.target_model : '',
    priority: typeof row.priority === 'number' ? row.priority : 100,
  }))
  const editor = useDocEditor(asArray(value), (doc) => {
    const rows = objectList(doc)
    const sources = rows.map((row) => String(row.source_model ?? ''))
    if (sources.some((source) => !source.trim())) return 'mappingRequired'
    if (rows.some((row) => !String(row.target_model ?? '').trim())) return 'mappingRequired'
    if (new Set(sources).size !== sources.length) return 'duplicateMappingSource'
    return undefined
  })
  const [rows, setRows] = useState<MappingDraft[]>(() => rowsOf(editor.rawDoc))
  const [nextId, setNextId] = useState(rows.length)
  useEffect(() => { setRows(rowsOf(editor.rawDoc)); setNextId((current) => Math.max(current, rowsOf(editor.rawDoc).length)) }, [editor.rawDoc])
  const write = (next: MappingDraft[]) => {
    setRows(next)
    editor.change(next.map((row) => ({ source_model: row.source, target_model: row.target, priority: row.priority === '' ? 100 : row.priority })))
  }
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('profileMappingsHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      <Group justify="space-between" align="center">
        <Text fw={600} size="sm">{t('modelMappings')}</Text>
        <Button variant="default" size="compact-sm" leftSection={<Plus size={15} />} onClick={() => write([...rows, { id: nextId, source: '', target: '', priority: 100 }])}>{t('addMapping')}</Button>
      </Group>
      {rows.length === 0 && <Text size="sm" c="dimmed">{t('modelMappingsEmpty')}</Text>}
      {rows.map((row, index) => (
        <Group key={row.id} gap="xs" align="flex-start" wrap="nowrap">
          <TextInput aria-label={`${t('mappingSource')} ${index + 1}`} className="mono-cell" placeholder={t('mappingSource')} value={row.source} onChange={(event) => write(rows.map((item) => item.id === row.id ? { ...item, source: event.currentTarget.value } : item))} style={{ flex: 1, minWidth: 0 }} />
          <TextInput aria-label={`${t('mappingTarget')} ${index + 1}`} className="mono-cell" placeholder={t('mappingTarget')} value={row.target} onChange={(event) => write(rows.map((item) => item.id === row.id ? { ...item, target: event.currentTarget.value } : item))} style={{ flex: 1, minWidth: 0 }} />
          <NumberInput aria-label={`${t('priority')} ${index + 1}`} min={0} hideControls value={row.priority} onChange={(next) => write(rows.map((item) => item.id === row.id ? { ...item, priority: next as number | '' } : item))} w={90} />
          <RemoveRowButton label={t('removeMapping')} index={index} onClick={() => write(rows.filter((item) => item.id !== row.id))} />
        </Group>
      ))}
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Key-profile allowed models: [{ pattern, match_type }].
// ---------------------------------------------------------------------------

export function AllowedModelsEditor({ name, label, value, setValid }: EditorProps) {
  const { t } = useTranslation()
  const rowsOf = (doc: Doc): Array<{ id: number; pattern: string; matchType: string }> => objectList(doc).map((row, index) => ({
    id: index,
    pattern: typeof row.pattern === 'string' ? row.pattern : '',
    matchType: row.match_type === 'regex' ? 'regex' : 'exact',
  }))
  const editor = useDocEditor(asArray(value), (doc) => {
    const rows = objectList(doc)
    const patterns = rows.map((row) => String(row.pattern ?? ''))
    if (patterns.some((pattern) => !pattern.trim())) return 'mappingRequired'
    if (new Set(patterns).size !== patterns.length) return 'duplicateAllowedModel'
    if (rows.some((row) => row.match_type === 'regex')) {
      for (const row of rows) {
        if (row.match_type === 'regex') {
          try { new RegExp(String(row.pattern)) } catch { return 'invalidRegex' }
        }
      }
    }
    return undefined
  })
  const [rows, setRows] = useState(() => rowsOf(editor.rawDoc))
  useEffect(() => { setRows(rowsOf(editor.rawDoc)) }, [editor.rawDoc])
  const [nextId, setNextId] = useState(rows.length)
  const write = (next: Array<{ id: number; pattern: string; matchType: string }>) => {
    setRows(next)
    editor.change(next.map((row) => ({ pattern: row.pattern, match_type: row.matchType })))
  }
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('allowedModelsHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      <Group justify="space-between" align="center">
        <Text fw={600} size="sm">{t('allowedModels')}</Text>
        <Button variant="default" size="compact-sm" leftSection={<Plus size={15} />} onClick={() => { write([...rows, { id: nextId, pattern: '', matchType: 'exact' }]); setNextId((current) => current + 1) }}>{t('addPattern')}</Button>
      </Group>
      {rows.length === 0 && <Text size="sm" c="dimmed">{t('allowedModelsEmpty')}</Text>}
      {rows.map((row, index) => (
        <Group key={row.id} gap="xs" align="flex-start" wrap="nowrap">
          <TextInput aria-label={`${t('modelPattern')} ${index + 1}`} className="mono-cell" placeholder={t('modelPattern')} value={row.pattern} onChange={(event) => write(rows.map((item) => item.id === row.id ? { ...item, pattern: event.currentTarget.value } : item))} style={{ flex: 1, minWidth: 0 }} />
          <Select
            aria-label={`${t('matchType')} ${index + 1}`}
            data={[{ value: 'exact', label: t('matchExact') }, { value: 'regex', label: t('matchRegex') }]}
            value={row.matchType}
            onChange={(matchType) => write(rows.map((item) => item.id === row.id ? { ...item, matchType: matchType || 'exact' } : item))}
            w={120}
          />
          <RemoveRowButton label={t('removeMapping')} index={index} onClick={() => write(rows.filter((item) => item.id !== row.id))} />
        </Group>
      ))}
    </DocEditorShell>
  )
}

// ---------------------------------------------------------------------------
// Association exclusions: { version, channel_ids, channel_tags,
// channel_name_patterns }. Channel ids come from the page's channel options so
// the operator picks real channels instead of pasting uuids.
// ---------------------------------------------------------------------------

export function ExclusionsEditor({ name, label, value, setValid, channels, channelsError, retryChannels }: EditorProps & {
  channels: Array<{ value: string; label: string }>
  channelsError?: boolean
  retryChannels?: () => void
}) {
  const { t } = useTranslation()
  const editor = useDocEditor(asObject(value).version === undefined ? { version: 1, ...asObject(value) } : asObject(value), () => undefined)
  const { doc } = editor
  const channelIds = stringList(doc.channel_ids)
  const tags = stringList(doc.channel_tags)
  const patterns = stringList(doc.channel_name_patterns)
  const configured = channelIds.length > 0 || tags.length > 0 || patterns.length > 0
  const write = (patch: Record<string, unknown>) => editor.change({ version: 1, ...doc, ...patch })
  useEffect(() => { setValid(editor.valid) }, [editor.valid, setValid])
  return (
    <DocEditorShell
      label={label}
      hint={t('exclusionsHint')}
      name={name}
      text={editor.text}
      mode={editor.mode}
      onModeChange={editor.setMode}
      onTextChange={editor.changeText}
      error={editor.error}
    >
      {!configured && <Text size="sm" c="dimmed">{t('exclusionsEmpty')}</Text>}
      <Select
        label={t('excludeChannelIds')}
        description={t('commaSeparated')}
        data={channels.map((channel) => ({ value: channel.value, label: channel.label }))}
        value={channelIds.length === 1 ? channelIds[0] : null}
        error={channelsError ? t('optionsUnavailable') : undefined}
        onChange={(id) => { if (id) write({ channel_ids: [...channelIds.filter((existing) => existing !== id), id] }) }}
        searchable
        clearable
        nothingFoundMessage={channels.length === 0 ? t('optionsUnavailable') : undefined}
      />
      {channelIds.length > 0 && (
        <Group gap="xs">
          {channelIds.map((id) => {
            const channel = channels.find((option) => option.value === id)
            return (
              <Button key={id} size="compact-sm" variant="light" aria-label={`${t('removeMapping')} ${channel?.label || id}`} onClick={() => write({ channel_ids: channelIds.filter((existing) => existing !== id) })}>
                {channel?.label || id} ×
              </Button>
            )
          })}
        </Group>
      )}
      <TagsInput label={t('excludeChannelTags')} description={t('commaSeparated')} value={tags} onChange={(next) => write({ channel_tags: next.map((tag) => tag.trim()).filter(Boolean) })} />
      <TagsInput label={t('excludeChannelNames')} description={t('excludeChannelNamesHint')} value={patterns} onChange={(next) => write({ channel_name_patterns: next.map((pattern) => pattern.trim()).filter(Boolean) })} />
    </DocEditorShell>
  )
}
