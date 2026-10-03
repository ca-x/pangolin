import { Code, Stack, Text } from '@mantine/core'
import { useTranslation } from 'react-i18next'
import { formatCount, formatMicros, UNMEASURED } from './observability'

export type MeasurementKey =
  | 'input_tokens'
  | 'output_tokens'
  | 'cache_read_tokens'
  | 'cache_write_tokens'
  | 'reasoning_tokens'
  | 'request_units'
  | 'image_input_tokens'
  | 'image_output_tokens'
export type UsageMeasurement = { version?: 1 } & Partial<Record<MeasurementKey, boolean>>
export type PricingFacts = {
  [key: string]: unknown
  pricing_status?: string | null
  settlement_kind?: string | null
  usage_measurement?: unknown
  cost_micros?: number | null
  total_cost_micros?: number | null
  cost_measured?: boolean
}
export type PricingCoverage = {
  missing_pricing_count?: number
  incomplete_usage_count?: number
  measured_cost_count?: number
  cost_measured?: boolean
  historical_amount_count?: number
  settled_cost_micros?: number | null
}
const KEYS: MeasurementKey[] = [
  'input_tokens',
  'output_tokens',
  'cache_read_tokens',
  'cache_write_tokens',
  'reasoning_tokens',
  'request_units',
  'image_input_tokens',
  'image_output_tokens',
]
export const nonnegative = (value: unknown): number | null =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null
export function measurement(value: unknown): UsageMeasurement | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const object = value as Record<string, unknown>
  if (
    object.version !== 1 ||
    Object.entries(object).some(
      ([key, item]) => key !== 'version' && (!KEYS.includes(key as MeasurementKey) || typeof item !== 'boolean'),
    )
  )
    return null
  return object as UsageMeasurement
}
export function quantityValue(row: PricingFacts, key: MeasurementKey, value: unknown): number | null {
  return measurement(row.usage_measurement)?.[key] === true ? nonnegative(value) : null
}
export function quantityText(row: PricingFacts, key: MeasurementKey, value: unknown) {
  return formatCount(quantityValue(row, key, value))
}
export function amountValue(
  row: PricingFacts,
  value: unknown = row.cost_micros ?? row.total_cost_micros,
): number | null {
  if (row.pricing_status == null && row.error_kind === 'usage_unavailable') return null
  if (
    row.pricing_status == null &&
    row.error_kind &&
    nonnegative(value) === 0 &&
    (nonnegative(row.input_tokens) ?? 0) + (nonnegative(row.output_tokens) ?? 0) === 0
  )
    return null
  return ['priced', 'explicit_free', 'legacy'].includes(row.pricing_status ?? 'legacy') ? nonnegative(value) : null
}
export function pricingKey(status: unknown) {
  return (
    (
      {
        priced: 'pricing_priced',
        explicit_free: 'pricing_explicit_free',
        missing_price: 'pricing_missing_price',
        incomplete_usage: 'pricing_incomplete_usage',
        legacy: 'pricing_legacy',
      } as Record<string, string>
    )[typeof status === 'string' ? status : 'legacy'] ?? 'pricing_unknown'
  )
}
export function settlementKey(kind: unknown) {
  return (
    (
      {
        reported: 'settlement_reported',
        conservative: 'settlement_conservative',
        unreported: 'settlement_unreported',
        duplicate: 'settlement_duplicate',
        interrupted: 'settlement_interrupted',
      } as Record<string, string>
    )[String(kind)] ?? 'settlement_unknown'
  )
}
export function CostFact({
  row,
  value,
  compact = false,
  showSettlement = true,
}: {
  row: PricingFacts
  value?: unknown
  compact?: boolean
  showSettlement?: boolean
}) {
  const { t } = useTranslation()
  const actual = value === undefined ? (row.cost_micros ?? row.total_cost_micros) : value
  const known = amountValue(row, actual)
  return (
    <Stack gap={2} style={{ minWidth: 0 }}>
      <Text component="span" size="md" style={{ fontVariantNumeric: 'tabular-nums' }}>
        {formatMicros(known)}
      </Text>
      {(!compact || known == null || row.pricing_status === 'legacy' || row.pricing_status == null) && (
        <Text component="span" size="md" c="dimmed">
          {t(pricingKey(row.pricing_status))}
        </Text>
      )}
      {showSettlement && row.settlement_kind != null && (!compact || row.settlement_kind !== 'reported') && (
        <Text component="span" size="md" c="dimmed">
          {t(settlementKey(row.settlement_kind))}
        </Text>
      )}
      {['conservative', 'interrupted'].includes(row.settlement_kind ?? '') && (
        <Text component="span" size="md" c="dimmed">
          {t('settledAmount')}: {formatMicros(actual)}
        </Text>
      )}
    </Stack>
  )
}
export function Settlement({ kind }: { kind: unknown }) {
  const { t } = useTranslation()
  return <>{t(settlementKey(kind))}</>
}
export function sumKnown(values: Array<number | null>): number | null {
  const known = values.filter((value): value is number => value != null)
  return known.length ? known.reduce((sum, value) => sum + value, 0) : null
}
export function sumQuantity<T extends PricingFacts>(rows: T[], key: MeasurementKey, read: (row: T) => unknown) {
  return sumKnown(rows.map((row) => quantityValue(row, key, read(row))))
}
export function sumAmounts<T extends PricingFacts>(rows: T[], read: (row: T) => unknown) {
  return sumKnown(rows.map((row) => amountValue(row, read(row))))
}
export function aggregateAmount(row: PricingCoverage, value: unknown) {
  return row.cost_measured === false ||
    row.measured_cost_count === 0 ||
    (row.cost_measured == null &&
      row.measured_cost_count == null &&
      (row as PricingCoverage & { usage_measured?: boolean }).usage_measured === false)
    ? null
    : nonnegative(value)
}
export function PricingCoverageNotice({
  coverage,
  historicalSummary = false,
}: {
  coverage: PricingCoverage
  historicalSummary?: boolean
}) {
  const { t } = useTranslation()
  const missing = nonnegative(coverage.missing_pricing_count)
  const incomplete = nonnegative(coverage.incomplete_usage_count)
  return (
    <Stack gap={2}>
      <Text size="md" c="dimmed">
        {historicalSummary
          ? t('summaryHistoricalHelp')
          : missing == null || incomplete == null
            ? t('pricingCountsUnknown')
            : missing + incomplete > 0
              ? t('partialPricing', { missing, incomplete })
              : t('measuredSubtotal')}
      </Text>
      {historicalSummary && missing != null && incomplete != null && missing + incomplete > 0 && (
        <Text size="md" c="dimmed">
          {t('partialPricing', { missing, incomplete })}
        </Text>
      )}
      {(coverage.historical_amount_count ?? 0) > 0 && (
        <Text size="md" c="dimmed">
          {t('pricingHistoricalIncluded', { count: coverage.historical_amount_count })}
        </Text>
      )}
      {coverage.settled_cost_micros != null && (
        <Text size="md" c="dimmed">
          {t('settledAmount')}: {formatMicros(coverage.settled_cost_micros)}
        </Text>
      )}
      {coverage.measured_cost_count != null && (
        <Text size="md" c="dimmed">
          {t(historicalSummary ? 'summaryIncludedRecords' : 'pricingMeasuredAttempts', {
            count: coverage.measured_cost_count,
          })}
        </Text>
      )}
    </Stack>
  )
}
export function SettlementFacts({ rows }: { rows: PricingFacts[] }) {
  const { t } = useTranslation()
  const ledger = (row: PricingFacts) => nonnegative(row.total_cost_micros ?? row.cost_micros)
  return (
    <Stack component="section" gap="sm" aria-label={t('settlementByExecution')}>
      <Text size="md" fw={560}>
        {t('settlementByExecution')}
      </Text>
      <Text size="md">
        {t('settledAmount')}: {formatMicros(sumKnown(rows.map(ledger)))}
      </Text>
      {rows.map((row, index) => (
        <Stack component="section" key={typeof row.execution_id === 'string' ? row.execution_id : index} gap={2}>
          <Text size="md">
            {t('executionId')}: <Code>{typeof row.execution_id === 'string' ? row.execution_id : UNMEASURED}</Code>
          </Text>
          <Text size="md">{t(settlementKey(row.settlement_kind))}</Text>
          <Text size="md" c="dimmed">
            {t('settledAmount')}: {formatMicros(ledger(row))}
          </Text>
        </Stack>
      ))}
    </Stack>
  )
}
export function rowCoverage(rows: PricingFacts[]): PricingCoverage {
  return {
    historical_amount_count: rows.filter(
      (r) => (r.pricing_status == null || r.pricing_status === 'legacy') && amountValue(r) != null,
    ).length,
    missing_pricing_count: rows.filter((r) => r.pricing_status === 'missing_price').length,
    incomplete_usage_count: rows.filter((r) => r.pricing_status === 'incomplete_usage').length,
    measured_cost_count: rows.filter((r) => ['priced', 'explicit_free'].includes(r.pricing_status ?? '')).length,
  }
}
export const costQuantityKey: Partial<Record<string, MeasurementKey>> = {
  input: 'input_tokens',
  output: 'output_tokens',
  cache_read: 'cache_read_tokens',
  cache_write: 'cache_write_tokens',
  reasoning: 'reasoning_tokens',
  unit: 'request_units',
  flat: 'request_units',
}
export type QuantityComponent = { [key: string]: unknown; kind?: unknown; cache_ttl?: unknown }
/** The stored component quantity may subtract child counters, even at zero rates. */
export function componentQuantity(
  row: PricingFacts | undefined,
  component: QuantityComponent | undefined,
  value: unknown,
  siblings: QuantityComponent[] | undefined,
): string {
  if (!row || !component || !siblings?.length || siblings.length > 64) return UNMEASURED
  const kind = typeof component.kind === 'string' ? component.kind : ''
  const key = costQuantityKey[kind]
  if (!key) return UNMEASURED
  const flags = measurement(row.usage_measurement)
  if (flags?.[key] !== true) return UNMEASURED
  // Lost component identities cannot prove the absence of a subtraction.
  if ((kind === 'input' || kind === 'output') && siblings.some((c) => !costQuantityKey[String(c.kind)]))
    return UNMEASURED
  const has = (child: string) => siblings.some((c) => c.kind === child)
  if (
    kind === 'input' &&
    ((has('cache_read') && flags.cache_read_tokens !== true) ||
      (has('cache_write') && flags.cache_write_tokens !== true))
  )
    return UNMEASURED
  if (kind === 'output' && has('reasoning') && flags.reasoning_tokens !== true) return UNMEASURED
  // V1 exposes only total cache writes, not the 1h subset or its 5m remainder.
  // Missing TTL identity is also insufficient to prove this is the total.
  if (kind === 'cache_write' && component.cache_ttl !== null) return UNMEASURED
  return formatCount(nonnegative(value))
}
