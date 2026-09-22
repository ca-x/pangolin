import { Alert, Anchor, Button, Group, Stack, Text } from '@mantine/core'
import { useQuery } from '@tanstack/react-query'
import { AlertTriangle, EyeOff, HelpCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { api, type Bootstrap, type RequestLoggingPolicy } from './api'
import i18n from './i18n'

/** Rendered wherever telemetry exists but carries no value. Never an invented zero. */
export const UNMEASURED = '—'

/**
 * One money formatter for the whole console. Pangolin stores money as integer
 * micro-USD, so six decimals is the exact scale of the record — printing fewer
 * digits truncates a real value, and printing the bare integer reads as dollars.
 * Negative values follow the "unmeasured" sentinel convention the latency
 * columns already use (`latency_ms >= 0`).
 */
export function formatMicros(micros: unknown): string {
  if (typeof micros !== 'number' || !Number.isFinite(micros) || micros < 0) return UNMEASURED
  return `$${new Intl.NumberFormat(i18n.language, { minimumFractionDigits: 6, maximumFractionDigits: 6 }).format(micros / 1_000_000)}`
}

/** Token and request counts. A measured zero is a real zero and prints as `0`. */
export function formatCount(value: unknown): string {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) return UNMEASURED
  return new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format(value)
}

export type ObservabilityReason =
  /** The projection is healthy and the logging policy records events. */
  | 'measured'
  /** The projection itself is down: the backend says so, so nothing can be read. */
  | 'unavailable'
  /** The projection is healthy but the logging policy is `off`, so no event is ever written. */
  | 'not_recorded'
  /** The policy could not be read at all, so whether anything is recorded is unknown. */
  | 'unknown'

export type Observability = {
  reason: ObservabilityReason
  /** True only when a zero really means "no traffic", not "nothing recorded". */
  measured: boolean
  policy: RequestLoggingPolicy | undefined
  refresh: () => void
}

/**
 * Resolves *why* the console has no numbers before it renders any of them. The
 * projection health is the backend's own verdict in `/api/v1/bootstrap`; the
 * recording state comes from the site request-logging policy, whose `off` level
 * (or a disabled policy) stops every request event at the source
 * (`src/operations/lifecycle.rs`). An unreported or unreadable value stays
 * unknown, so a real zero never turns into a false alarm.
 *
 * "Unreadable" is the third case and not a footnote: `/settings/request-logging`
 * is instance-owner-only, so a project member's read is refused while
 * `/operations` stays in their navigation. Reading that refusal as "logging is
 * on" printed measured zeros for telemetry the console never saw.
 */
export function useObservability(): Observability {
  const bootstrap = useQuery({ queryKey: ['bootstrap'], queryFn: () => api<Bootstrap>('/api/v1/bootstrap'), retry: false, staleTime: 300_000 })
  const policy = useQuery({ queryKey: ['request-logging-policy'], queryFn: () => api<RequestLoggingPolicy>('/api/admin/v1/settings/request-logging'), retry: false, staleTime: 300_000 })
  const available = bootstrap.data?.observability_available !== false
  const value = policy.data && typeof policy.data === 'object' && !Array.isArray(policy.data) ? policy.data : undefined
  const loggingOff = value?.enabled === false || value?.default_level === 'off'
  // A failed read, or a body that is not a policy at all, is unknown — never "on".
  const unreadable = policy.isError || (policy.isSuccess && value === undefined)
  const reason: ObservabilityReason = !available ? 'unavailable' : unreadable ? 'unknown' : loggingOff ? 'not_recorded' : 'measured'
  return {
    reason,
    measured: reason === 'measured',
    policy: value,
    refresh: () => { void bootstrap.refetch(); void policy.refetch() },
  }
}

/**
 * A persistent notice, not a dismissible toast: while telemetry is unmeasured
 * every number on the page is unknown, and that has to stay visible next to
 * them. `unavailable` is an outage of the projection, `not_recorded` is a
 * configuration choice, so the copy and the offered action differ.
 */
export function ObservabilityNotice({ observability, onRetry }: { observability: Observability; onRetry?: () => void }) {
  const { t } = useTranslation()
  if (observability.measured) return null
  const unavailable = observability.reason === 'unavailable'
  // An unreadable policy is a third state with its own copy: it is not an outage
  // and it is not a configuration choice, so it must not borrow either sentence.
  const unknown = observability.reason === 'unknown'
  const title = unavailable ? t('telemetryUnavailableTitle') : unknown ? t('telemetryUnknownTitle') : t('telemetryNotRecordedTitle')
  const copy = unavailable ? t('telemetryUnavailableCopy') : unknown ? t('telemetryUnknownCopy') : t('telemetryNotRecordedCopy')
  return <Alert
    variant="light"
    color={unavailable ? 'red' : unknown ? 'gray' : 'yellow'}
    radius="lg"
    role="alert"
    icon={unavailable ? <AlertTriangle /> : unknown ? <HelpCircle /> : <EyeOff />}
    title={title}
    mb="lg"
  >
    <Group justify="space-between" align="flex-end" gap="md" wrap="wrap">
      <Stack gap={4} style={{ flex: '1 1 320px' }}>
        <Text size="sm">{copy}</Text>
        <Text size="xs" c="dimmed">{t('telemetryUnmeasuredHint')}</Text>
      </Stack>
      {unavailable || unknown
        ? onRetry && <Button variant="light" color={unavailable ? 'red' : 'gray'} size="compact-sm" onClick={onRetry}>{t('retry')}</Button>
        : <Anchor component={Link} to="/system" size="sm">{t('requestLogging')}</Anchor>}
    </Group>
  </Alert>
}

/**
 * A failed request carries `error_kind` — the reason — next to the upstream status
 * the gateway actually observed, which is `null` when there was none to observe.
 * The kind is the actionable half, so it is translated; an unknown kind is shown
 * verbatim rather than hidden.
 */
export function errorKindKey(kind: string): string | null {
  return ({ failed: 'errorKindFailed', local_failure: 'errorKindLocalFailure', usage_unavailable: 'errorKindUsageUnavailable', cancelled: 'errorKindCancelled', interrupted: 'errorKindInterrupted' } as Record<string, string>)[kind] ?? null
}

/**
 * The record's own machine codes for a job, schedule or probe failure. They are not
 * copy, so the console names the ones it knows in the active language and shows
 * anything else verbatim rather than hiding it.
 */
export function errorCodeKey(code: string): string | null {
  return ({ invalid_configuration: 'errorCodeInvalidConfiguration', attempts_exhausted: 'errorCodeAttemptsExhausted', operation_failed: 'errorCodeOperationFailed', probe_failed: 'errorCodeProbeFailed' } as Record<string, string>)[code] ?? null
}

/** The localized name of a recorded code, resolved through `t`. */
export function useErrorCodeLabel() {
  const { t } = useTranslation()
  return (code: string) => {
    const key = errorCodeKey(code)
    return key ? t(key) : code
  }
}

/** The localized reason for a failure, resolved through `t` so it follows the active language. */
export function useErrorKindLabel() {
  const { t } = useTranslation()
  return (kind: string) => {
    const key = errorKindKey(kind)
    return key ? t(key) : kind
  }
}

/**
 * Whether the record actually measured a request's usage. The projection settles
 * every attempt, and an attempt the upstream rejected settles as a row of zeros —
 * so a bare `0` on a failure is the gateway's normalized rejection, not a
 * measurement. `usage_unavailable` is the backend's own verdict that the
 * terminal usage report was lost; a failure that recorded no tokens and no cost
 * is the same fact from the other direction. A partial settlement — a stream
 * that broke after it had reported usage — keeps its real numbers.
 */
export function usageMeasured(row: { error_kind?: string | null; input_tokens?: number | null; output_tokens?: number | null; cost_micros?: number | null }): boolean {
  if (!row.error_kind) return true
  if (row.error_kind === 'usage_unavailable') return false
  return (row.input_tokens ?? 0) + (row.output_tokens ?? 0) > 0 || (row.cost_micros ?? 0) > 0
}
