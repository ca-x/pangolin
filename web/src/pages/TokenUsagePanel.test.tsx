import { render, screen, within } from '@testing-library/react'
import { beforeEach, expect, it } from 'vitest'
import i18n from '../i18n'
import { ThemeProvider } from '../theme'
import type { AnalyticsRow } from '../api'
import { TokenUsagePanel } from './TokenUsagePanel'

beforeEach(async () => { await i18n.changeLanguage('en') })

it('shows native measured subtotals, paired coverage and true zero without inferring missing totals', async () => {
  const common = { requests: 4, attempts: 4, errors: 0, cache_savings_micros: null, cost_micros: null, latency_ms: null, ttft_ms: null }
  const rows: AnalyticsRow[] = [
    { ...common, dimension: 'measured', input_tokens: 17, output_tokens: 16, total_tokens: 15, total_tokens_samples: 2, cache_hit_tokens: 2, cache_write_tokens: 3, reasoning_tokens: 1 },
    { ...common, dimension: 'unknown', input_tokens: 10, output_tokens: 12, cache_hit_tokens: null },
    { ...common, dimension: 'zero', input_tokens: 0, output_tokens: 0, total_tokens: 0, total_tokens_samples: 4, cache_hit_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0 },
  ]
  render(<ThemeProvider><TokenUsagePanel rows={rows} label={(value) => ({ text: value ?? '—', opaque: false })} /></ThemeProvider>)
  const region = screen.getByRole('region', { name: 'Token usage' })
  expect(region).toHaveAttribute('tabindex', '0')
  const measured = within(region).getByRole('row', { name: /^measured\b/ })
  expect(within(measured).getByText('15')).toBeInTheDocument()
  expect(within(measured).queryByText('33')).not.toBeInTheDocument()
  expect(within(measured).getByText('2 / 4 measured attempts')).toBeInTheDocument()
  const unknown = within(region).getByRole('row', { name: /unknown/ })
  expect(within(unknown).queryByText('22')).not.toBeInTheDocument()
  expect(within(unknown).getAllByText('—')).toHaveLength(4)
  const zero = within(region).getByRole('row', { name: /zero/ })
  expect(within(zero).getAllByText('0')).toHaveLength(6)
  await i18n.changeLanguage('zh-CN')
  expect(await screen.findByRole('heading', { name: 'Token 用量' })).toBeInTheDocument()
  expect(screen.getByText('2 / 4 次尝试已测量')).toBeInTheDocument()
})
