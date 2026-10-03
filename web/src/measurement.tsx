import { Stack, Text } from '@mantine/core'
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
  const actual = value === undefined ? row.cost_micros ?? row.total_cost_micros : value
  const known = amountValue(row, actual)
  return (
    <Stack gap={2} style={{ minWidth: 0 }}>
      <Text component="span" size="sm" style={{ fontVariantNumeric: 'tabular-nums' }}>
        {formatMicros(known)}
      </Text>
      {(!compact || known == null || row.pricing_status === 'legacy' || row.pricing_status == null) && (
        <Text component="span" size="sm" c="dimmed">
          {t(pricingKey(row.pricing_status))}
        </Text>
      )}
      {showSettlement && row.settlement_kind != null && (!compact || row.settlement_kind !== 'reported') && (
        <Text component="span" size="sm" c="dimmed">
          {t(settlementKey(row.settlement_kind))}
        </Text>
      )}
      {['conservative', 'interrupted'].includes(row.settlement_kind ?? '') && (
        <Text component="span" size="sm" c="dimmed">
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
export function PricingCoverageNotice({ coverage }: { coverage: PricingCoverage }) {
  const { t } = useTranslation()
  const missing = nonnegative(coverage.missing_pricing_count)
  const incomplete = nonnegative(coverage.incomplete_usage_count)
  return (
    <Stack gap={2}>
      <Text size="sm" c="dimmed">
        {missing == null || incomplete == null
          ? t('pricingCountsUnknown')
          : missing + incomplete > 0
            ? t('partialPricing', { missing, incomplete })
            : t('measuredSubtotal')}
      </Text>
      {(coverage.historical_amount_count ?? 0) > 0 && (
        <Text size="sm" c="dimmed">
          {t('pricingHistoricalIncluded', { count: coverage.historical_amount_count })}
        </Text>
      )}
      {coverage.settled_cost_micros != null && (
        <Text size="sm" c="dimmed">
          {t('settledAmount')}: {formatMicros(coverage.settled_cost_micros)}
        </Text>
      )}
      {coverage.measured_cost_count != null && (
        <Text size="sm" c="dimmed">
          {t('pricingMeasuredAttempts', { count: coverage.measured_cost_count })}
        </Text>
      )}
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
export function componentQuantity(row: PricingFacts | undefined, kind: unknown, value: unknown): string {
  if (!row) return UNMEASURED
  const key = costQuantityKey[String(kind)]
  return key ? quantityText(row, key, value) : UNMEASURED
}
