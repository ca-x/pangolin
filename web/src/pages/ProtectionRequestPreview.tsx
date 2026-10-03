import { Alert, Badge, Button, Code, Group, Modal, Paper, Select, Stack, Tabs, Text, Textarea } from '@mantine/core'
import { AlertTriangle, Eye } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useProject } from '../project'

const ENDPOINTS = [
  '/v1/chat/completions',
  '/v1/completions',
  '/v1/responses',
  '/v1/responses/compact',
  '/v1/messages',
  '/v1/embeddings',
  '/v1/moderations',
  '/v1/alpha/search',
  '/v1/images/generations',
  '/v1/images/edits',
  '/v1/videos',
  '/v1/audio/speech',
  '/v1/audio/transcriptions',
  '/v1/audio/translations',
  '/v1/rerank',
  '/v1beta/models:generateContent',
  '/v1beta/models:streamGenerateContent',
  '/doubao/v3/contents/generations/tasks',
]
const REASONS = [
  'matched',
  'allowlisted',
  'role_mismatch',
  'scope_mismatch',
  'disabled',
  'inactive',
  'test_mode',
  'no_match',
] as const
export const utf8Size = (value: string) => new TextEncoder().encode(value).byteLength
function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}
type Finding = {
  rule_id: string
  path: string
  start: number
  end: number
  action: 'deny' | 'redact'
  reason: (typeof REASONS)[number]
}
type Preview = {
  decision: 'allow' | 'redact' | 'deny'
  redacted_body: Record<string, unknown>
  findings: Finding[]
  suppressed_findings: Finding[]
}
type TextRule = {
  id: string
  name: string
  description: string
  action: string
  enabled: boolean
  state: string
  matched: boolean
  result: string
}
function findings(value: unknown): value is Finding[] {
  return (
    Array.isArray(value) &&
    value.length <= 512 &&
    value.every(
      (f) =>
        object(f) &&
        typeof f.rule_id === 'string' &&
        f.rule_id.length <= 128 &&
        typeof f.path === 'string' &&
        utf8Size(f.path) <= 4096 &&
        Number.isSafeInteger(f.start) &&
        Number.isSafeInteger(f.end) &&
        Number(f.start) >= 0 &&
        Number(f.end) >= Number(f.start) &&
        ['deny', 'redact'].includes(String(f.action)) &&
        REASONS.includes(f.reason as Finding['reason']),
    )
  )
}
function preview(value: unknown): value is Preview {
  return (
    object(value) &&
    ['allow', 'redact', 'deny'].includes(String(value.decision)) &&
    object(value.redacted_body) &&
    utf8Size(JSON.stringify(value.redacted_body)) <= 65536 &&
    findings(value.findings) &&
    findings(value.suppressed_findings) &&
    value.findings.length + value.suppressed_findings.length <= 512 &&
    value.truncated === false
  )
}
export function ProtectionRequestPreview() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [openedProject, setOpenedProject] = useState<string | null>(null)
  useEffect(() => {
    setOpenedProject(null)
  }, [project.id])
  return (
    <>
      <Group justify="flex-end" mb="md">
        <Button variant="default" leftSection={<Eye size={17} />} onClick={() => setOpenedProject(project.id)}>
          {t('previewRules')}
        </Button>
      </Group>
      {openedProject === project.id && (
        <PreviewSession key={project.id} projectId={project.id} onClose={() => setOpenedProject(null)} />
      )}
    </>
  )
}
/** No sample, variable or result enters a query/mutation observer or cache. */
function PreviewSession({ projectId, onClose }: { projectId: string; onClose: () => void }) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<'text' | 'request'>('text')
  const [sample, setSample] = useState('')
  const [endpoint, setEndpoint] = useState(ENDPOINTS[0])
  const [result, setResult] = useState<Preview | null>(null)
  const [textRules, setTextRules] = useState<TextRule[] | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState(false)
  const life = useRef({ alive: true, generation: 0, controller: null as AbortController | null })
  useLayoutEffect(() => {
    const current = life.current
    current.alive = true
    return () => {
      current.alive = false
      current.generation++
      current.controller?.abort()
    }
  }, [])
  const invalidate = () => {
    life.current.generation++
    life.current.controller?.abort()
    life.current.controller = null
    setResult(null)
    setTextRules(null)
    setPending(false)
    setError(false)
  }
  const change = (value: string) => {
    invalidate()
    setSample(value)
  }
  const size = utf8Size(sample)
  let body: Record<string, unknown> | null = null
  let validation: string | undefined
  if (mode === 'request') {
    if (size > 65536) validation = t('privacySizeError')
    else if (sample) {
      try {
        const parsed: unknown = JSON.parse(sample)
        if (!object(parsed)) throw Error()
        body = parsed
        if (utf8Size(JSON.stringify(body)) > 65536) validation = t('privacySizeError')
      } catch {
        validation = t('privacyObjectError')
      }
    }
  } else if (size > 16384) validation = t('sampleTextTooLarge')
  const run = async (event?: FormEvent) => {
    event?.preventDefault()
    if (!sample || validation || (mode === 'request' && !body)) return
    invalidate()
    const generation = life.current.generation
    const controller = new AbortController()
    life.current.controller = controller
    setPending(true)
    try {
      const value: unknown = await api(
        `/api/admin/v1/projects/${encodeURIComponent(projectId)}/${mode === 'request' ? 'protection-request-preview' : 'protection-preview'}`,
        {
          method: 'POST',
          signal: controller.signal,
          body: JSON.stringify(mode === 'request' ? { endpoint, body } : { text: sample }),
        },
      )
      if (!life.current.alive || generation !== life.current.generation) return
      if (mode === 'request') {
        if (!preview(value)) throw Error()
        setResult(value)
      } else {
        if (!object(value) || !Array.isArray(value.rules) || value.rules.length > 500) throw Error()
        setTextRules(value.rules as TextRule[])
      }
    } catch {
      if (life.current.alive && generation === life.current.generation) setError(true)
    } finally {
      if (life.current.alive && generation === life.current.generation) {
        life.current.controller = null
        setPending(false)
      }
    }
  }
  const close = () => {
    invalidate()
    setSample('')
    onClose()
  }
  return (
    <Modal
      opened
      onClose={close}
      title={t('protectionPreviewTitle')}
      size="lg"
      closeButtonProps={{ 'aria-label': t('close') }}
      classNames={{ content: 'protection-preview-dialog' }}
    >
      <Stack gap="md" style={{ minWidth: 0 }}>
        <Text size="sm" c="dimmed">
          {t('privacyTransient')}
        </Text>
        <Tabs
          value={mode}
          onChange={(value) => {
            invalidate()
            setSample('')
            setMode(value === 'request' ? 'request' : 'text')
          }}
        >
          <Tabs.List>
            <Tabs.Tab value="text">{t('privacyText')}</Tabs.Tab>
            <Tabs.Tab value="request">{t('privacyRequest')}</Tabs.Tab>
          </Tabs.List>
        </Tabs>
        <form onSubmit={(event) => void run(event)}>
          <Stack gap="md">
            {mode === 'request' && (
              <>
                <Select
                  label={t('privacyEndpoint')}
                  value={endpoint}
                  allowDeselect={false}
                  data={ENDPOINTS.map((value) => ({
                    value,
                    label:
                      value === '/v1/responses'
                        ? t('previewProtocolResponses')
                        : value === '/v1/messages'
                          ? t('previewProtocolMessages')
                          : value === '/v1/chat/completions'
                            ? t('previewProtocolChat')
                            : value,
                  }))}
                  onChange={(value) => {
                    invalidate()
                    setEndpoint(value || ENDPOINTS[0])
                  }}
                />
                <Text size="sm" c="dimmed">
                  {t('privacyContextHelp')}
                </Text>
              </>
            )}
            <Textarea
              label={t(mode === 'request' ? 'privacyJson' : 'sampleText')}
              description={mode === 'text' ? t('sampleTextHint') : undefined}
              value={sample}
              onChange={(event) => change(event.currentTarget.value)}
              rows={7}
              required
              error={validation}
              styles={{ input: { fontFamily: 'var(--font-mono)', fontSize: 14 } }}
            />
            <Group justify="flex-end">
              <Button type="submit" loading={pending} disabled={!sample || Boolean(validation)}>
                {t('runPreview')}
              </Button>
            </Group>
          </Stack>
        </form>
        <div aria-live="polite">
          {pending && (
            <Text role="status" size="sm">
              {t('previewLoading')}
            </Text>
          )}
          {error && (
            <Alert color="red" title={t('previewError')} icon={<AlertTriangle size={18} />}>
              <Stack gap="sm">
                <Text size="sm">{t('privacyFailure')}</Text>
                <Button variant="outline" color="red" onClick={() => void run()}>
                  {t('retry')}
                </Button>
              </Stack>
            </Alert>
          )}
          {result && (
            <Stack gap="md">
              <Group>
                <Text fw={600}>{t('privacyDecision')}</Text>
                <Badge color={result.decision === 'deny' ? 'red' : result.decision === 'redact' ? 'yellow' : 'teal'}>
                  {t(`privacy_${result.decision}`)}
                </Badge>
              </Group>
              <Text size="sm" c="dimmed">
                {t('privacyOffsetsHelp')}
              </Text>
              <FindingList title={t('privacyFindings')} rows={result.findings} />
              <FindingList title={t('privacySuppressed')} rows={result.suppressed_findings} />
              <Text fw={560}>{t('privacyResult')}</Text>
              <Text
                component="pre"
                aria-label={t('privacyResult')}
                className="protection-preview-result"
                style={{
                  overflow: 'auto',
                  maxHeight: 320,
                  whiteSpace: 'pre-wrap',
                  overflowWrap: 'anywhere',
                  minWidth: 0,
                  fontSize: 14,
                }}
              >
                {JSON.stringify(result.redacted_body, null, 2)}
              </Text>
            </Stack>
          )}
          {textRules && (
            <Stack gap="sm" role="list" aria-label={t('protectionPreviewResults')}>
              {!textRules.length && <Text size="sm">{t('protectionPreviewEmpty')}</Text>}
              {textRules.map((rule) => (
                <Paper withBorder p="md" key={rule.id} role="listitem">
                  <Stack gap="xs">
                    <Text fw={600}>{rule.name}</Text>
                    {rule.description && <Text size="sm">{rule.description}</Text>}
                    <Text size="sm">
                      <span>{t(rule.matched ? 'matched' : 'noMatch')}</span> · <span>{t(rule.action === 'deny' ? 'deny' : 'redact')}</span> ·{' '}
                      <span>{t(rule.enabled ? 'enabled' : 'disabled')}</span> ·{' '}
                      <span>{t(rule.state === 'archived' ? 'archived' : 'active')}</span>
                    </Text>
                    {rule.matched && (
                      <>
                        <Text fw={500}>{t('previewResult')}</Text>
                        {rule.action === 'deny' && <Text size="sm">{t('previewDenyHint')}</Text>}
                        <Text
                          component="pre"
                          className="protection-preview-result"
                          style={{ maxHeight: 280, overflow: 'auto', whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}
                        >
                          {rule.result}
                        </Text>
                      </>
                    )}
                  </Stack>
                </Paper>
              ))}
            </Stack>
          )}
        </div>
      </Stack>
    </Modal>
  )
}
function FindingList({ title, rows }: { title: string; rows: Finding[] }) {
  const { t } = useTranslation()
  return (
    <section aria-label={title}>
      <Text fw={560} mb="xs">
        {title}
      </Text>
      {!rows.length ? (
        <Text size="sm" c="dimmed">
          {t('privacyEmpty')}
        </Text>
      ) : (
        <Stack gap="xs">
          {rows.map((f, index) => (
            <Paper key={index} withBorder p="sm">
              <Stack gap={4}>
                <Text size="sm">
                  {t(`privacy_${f.reason}`)} · {t(f.action)}
                </Text>
                <Text size="sm">
                  {t('privacyRule')}: <Code>{f.rule_id}</Code>
                </Text>
                {f.path && (
                  <>
                    <Code style={{ whiteSpace: 'normal', overflowWrap: 'anywhere', fontSize: 14 }}>{f.path}</Code>
                    <Text size="sm">{t('privacyBytes', { start: f.start, end: f.end })}</Text>
                  </>
                )}
              </Stack>
            </Paper>
          ))}
        </Stack>
      )}
    </section>
  )
}
