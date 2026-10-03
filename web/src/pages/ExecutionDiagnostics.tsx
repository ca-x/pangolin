import { Button, Code, Group, Stack, Text } from '@mantine/core'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Modal } from '../components'
import { formatDate } from './shared'
const CODES = [
  'unsupported_request_shape',
  'unsupported_response_shape',
  'unsupported_tools',
  'unsupported_nontext_content',
  'unsupported_streaming',
  'roles_normalized',
  'stop_array_normalized',
  'text_parts_normalized',
]
const PATHS = [
  'tools',
  'tool_choice',
  'messages',
  'messages[].content',
  'messages[].role',
  'messages[].tool_calls',
  'input',
  'stream',
  'system',
  'stop',
  'contents[].parts',
  'choices[].message',
  'candidates[].content.parts',
  'content',
  'response',
  'generationConfig',
  'choices',
]
const REASONS = ['first', 'hit', 'expired', 'ineligible', 'established', 'released', 'candidate_failed']
const id = (value: unknown) => typeof value === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(value)
function object(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === 'object' && !Array.isArray(value)
}
function expiryText(value: number): string {
  if (!Number.isFinite(new Date(value * 1000).getTime())) return '—'
  return formatDate(value)
}
export type DiagnosticAttempt = {
  id?: string
  attempt?: number | null
  conversion_diagnostics?: unknown
  affinity_diagnostics?: unknown
}
export function ExecutionDiagnostics({ attempt }: { attempt: DiagnosticAttempt }) {
  const { t } = useTranslation()
  const rawConversion = attempt.conversion_diagnostics
  const rawAffinity = attempt.affinity_diagnostics
  const conversion =
    Array.isArray(rawConversion) &&
    rawConversion.length <= 64 &&
    rawConversion.every(
      (d) =>
        object(d) &&
        Object.keys(d).every((k) => ['phase', 'code', 'reason', 'path'].includes(k)) &&
        ['request', 'response'].includes(String(d.phase)) &&
        CODES.includes(String(d.code)) &&
        d.reason === d.code &&
        (d.path == null || PATHS.includes(String(d.path))),
    )
      ? (rawConversion as Array<{ phase: string; code: string; path?: string }>)
      : []
  const affinity =
    Array.isArray(rawAffinity) &&
    rawAffinity.length <= 16 &&
    rawAffinity.every(
      (d) =>
        object(d) &&
        Object.keys(d).every((k) => ['rule_id', 'scope_digest', 'reason', 'provider_id', 'expires_at'].includes(k)) &&
        id(d.rule_id) &&
        typeof d.scope_digest === 'string' &&
        /^[a-fA-F0-9]{64}$/.test(d.scope_digest) &&
        REASONS.includes(String(d.reason)) &&
        (d.provider_id == null || id(d.provider_id)) &&
        (d.expires_at == null || (Number.isSafeInteger(d.expires_at) && Number(d.expires_at) >= 0)),
    )
      ? (rawAffinity as Array<{ rule_id: string; reason: string; provider_id?: string; expires_at?: number }>)
      : []
  return (
    <section aria-label={`${t('attempt')} #${attempt.attempt ?? '—'}`} style={{ minWidth: 0 }}>
      <Stack gap="xs">
        <Text fw={560}>
          {t('attempt')} #{attempt.attempt ?? '—'}
        </Text>
        {!conversion.length && !affinity.length && (
          <Text size="sm" c="dimmed">
            {t('noDiagnostics')}
          </Text>
        )}
        {conversion.map((d, index) => (
          <Group key={`c${index}`} gap="xs" wrap="wrap">
            <Text size="sm" c="dimmed">
              {t(`conversionPhase_${d.phase}`)}
            </Text>
            <Text size="sm">{t(`conversion_${d.code}`)}</Text>
            {d.path && <Code style={{ whiteSpace: 'normal', overflowWrap: 'anywhere', fontSize: 14 }}>{d.path}</Code>}
          </Group>
        ))}
        {affinity.map((d, index) => (
          <Stack key={`a${index}`} gap={2}>
            <Text size="sm">
              {t('affinityDiagnostics')}: {t(`affinity_${d.reason}`)}
            </Text>
            <Text size="sm">
              {t('affinityRule')}: <Code>{d.rule_id}</Code>
            </Text>
            {d.provider_id && (
              <Text size="sm">
                {t('affinityChannel')}: <Code style={{ overflowWrap: 'anywhere' }}>{d.provider_id}</Code>
              </Text>
            )}
            {d.expires_at != null && (
              <Text size="sm">
                {t('affinityExpiry')}: {expiryText(d.expires_at)}
              </Text>
            )}
          </Stack>
        ))}
      </Stack>
    </section>
  )
}
export function AttemptDiagnosticList({ executions }: { executions: DiagnosticAttempt[] }) {
  const { t } = useTranslation()
  return (
    <Stack gap="md" component="section" aria-label={t('attemptDiagnostics')}>
      <Text fw={600}>{t('attemptDiagnostics')}</Text>
      {executions.length ? (
        executions.map((attempt, index) => <ExecutionDiagnostics key={attempt.id ?? index} attempt={attempt} />)
      ) : (
        <Text size="sm" c="dimmed">
          {t('executionEmpty')}
        </Text>
      )}
    </Stack>
  )
}
export function ExecutionDiagnosticsButton({ attempt }: { attempt: DiagnosticAttempt }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <>
      <Button size="compact-sm" variant="subtle" onClick={() => setOpen(true)}>
        {t('attemptDiagnostics')}
      </Button>
      <Modal open={open} onOpenChange={setOpen} title={t('attemptDiagnostics')}>
        {open && <ExecutionDiagnostics attempt={attempt} />}
      </Modal>
    </>
  )
}
