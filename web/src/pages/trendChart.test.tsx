import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import OverviewPage from './OverviewPage'

/**
 * The trend carries four series: requests and errors on their count scale, and
 * total tokens and cost as small multiples with their own labelled scales. These
 * tests pin the rules that keep the picture honest — one bucket is a level line
 * and never a triangle to zero, an unmeasured bucket is a gap rather than a zero,
 * a measured zero is plotted as the zero it is, and no unit is normalised onto an
 * axis that belongs to another one.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const bootstrap = { initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, observability_available: true, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }

const mockApi = (series: unknown[], summary: Record<string, unknown> = {}) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json(bootstrap)
  if (path.includes('settings/request-logging')) return json({ enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false })
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/observability/summary')) return json({ requests: 5, errors: 1, error_rate: 0.2, p95_latency_ms: 10, input_tokens: 5, output_tokens: 5, cost_micros: 0, series, ...summary })
  if (path.includes('/observability/requests')) return json({ data: [], total: 0 })
  return json({ data: [], total: 0 })
})

const renderPage = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><OverviewPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

const paths = (selector: string) => [...document.querySelectorAll(selector)].map((node) => node.getAttribute('d') ?? '')
const cells = (index: number) => [...document.querySelectorAll('.chart-table tbody tr')[index].querySelectorAll('td')].map((cell) => cell.textContent)

describe('the trend chart tells the truth about its buckets', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('draws a single bucket as a level line, not a triangle to zero', async () => {
    vi.stubGlobal('fetch', mockApi([{ bucket: 1700000000, requests: 5, errors: 1, latency_ms: 10, input_tokens: 20, output_tokens: 10, cost_micros: 4 }]))
    renderPage()
    await screen.findByText('1 failed')
    const area = document.querySelector('path.chart-area')!.getAttribute('d')!
    const line = document.querySelector('path.chart-line')!.getAttribute('d')!
    const top = area.match(/^M (\S+) (\S+) L (\S+) (\S+)/)!
    // The top edge is level and spans the plot; the old path fell from x=0 to
    // (640, 240), which is what drew the triangle.
    expect(Number(top[2])).toBe(Number(top[4]))
    expect(Number(top[1])).toBe(0)
    expect(Number(top[3])).toBe(640)
    expect(line).toMatch(/^M 0\.00 ([\d.]+) L 640\.00 \1$/)
    // The one measurement is plotted in the middle, where its tooltip can be found.
    expect(document.querySelector('circle.chart-dot')?.getAttribute('cx')).toBe('320')
    // Tokens and cost are one measurement each too: a level line, never a fall to
    // the baseline, which would read as a collapse in a unit nobody reported.
    const sparks = paths('path.chart-spark-line')
    expect(sparks).toHaveLength(2)
    for (const spark of sparks) expect(spark).toMatch(/^M 0\.00 ([\d.]+) L 640\.00 \1$/)
  })

  it('still draws a line between two real buckets', async () => {
    vi.stubGlobal('fetch', mockApi([{ bucket: 1700000000, requests: 2, errors: 0, latency_ms: 10 }, { bucket: 1700003600, requests: 5, errors: 1, latency_ms: 10 }]))
    renderPage()
    await screen.findByText('1 failed')
    const line = document.querySelector('path.chart-line')!.getAttribute('d')!
    expect(line).toMatch(/^M 0\.00 \S+ L 640\.00 \S+$/)
    expect(Number(line.match(/L 640\.00 (\S+)/)![1])).toBeLessThan(Number(line.match(/M 0\.00 (\S+)/)![1]))
  })

  it('gives tokens and cost their own scales instead of one axis for every unit', async () => {
    vi.stubGlobal('fetch', mockApi([
      { bucket: 1700000000, requests: 2, errors: 0, latency_ms: 10, input_tokens: 900000, output_tokens: 100000, cost_micros: 500000 },
      { bucket: 1700003600, requests: 4, errors: 1, latency_ms: 10, input_tokens: 1000000, output_tokens: 200000, cost_micros: 900000 },
    ]))
    renderPage()
    await screen.findByText('1 failed')
    // The count plot's own axis is the request count. Had a token total been
    // normalised onto it, its top label would read in the millions.
    const countLabels = [...document.querySelectorAll('.chart-y-labels .chart-axis-label')].map((node) => node.textContent)
    expect(countLabels[0]).toBe('6')
    const heads = [...document.querySelectorAll('.chart-spark')].map((row) => row.textContent)
    // Each row names its unit and its own range, so a reader is never asked to
    // read micro-USD off the request axis.
    expect(heads[0]).toContain('Total tokens')
    expect(heads[0]).toContain('0 – 1.6M')
    expect(heads[1]).toContain('Cost')
    expect(heads[1]).toContain('0 – $1.000000')
  })

  it('leaves an unmeasured bucket as a gap and plots a measured zero as zero', async () => {
    vi.stubGlobal('fetch', mockApi([
      { bucket: 1700000000, requests: 2, errors: 0, latency_ms: 10, input_tokens: 10, output_tokens: 5, cost_micros: 3 },
      { bucket: 1700003600, requests: 1, errors: 0, latency_ms: 10, input_tokens: null, output_tokens: null, cost_micros: null },
      { bucket: 1700007200, requests: 3, errors: 0, latency_ms: 10, input_tokens: 0, output_tokens: 0, cost_micros: 0 },
    ]))
    renderPage()
    await screen.findByText('Request trend')
    const token = paths('path.chart-spark-line')[0]
    // Two runs: the measured hour, then the measured zero. The unmeasured hour is
    // in neither — it is a gap, not a point on the baseline.
    expect(token.match(/M /g)).toHaveLength(2)
    // An isolated measurement has no slope to draw, so it keeps its dot on the
    // baseline (a measured zero is plotted as the zero it is).
    expect(token).toContain('M 640.00 56.00')
    expect(token).not.toContain('320.00')
    // The disclosure table prints the same distinction: `—` where nothing was
    // measured, `0` where a zero was measured.
    expect(cells(0).slice(3)).toEqual(['10', '5', '15', '$0.000003'])
    expect(cells(1).slice(3)).toEqual(['—', '—', '—', '—'])
    expect(cells(2).slice(3)).toEqual(['0', '0', '0', '$0.000000'])
  })

  it('says unmeasured instead of zero when the window measured no usage at all', async () => {
    vi.stubGlobal('fetch', mockApi(
      [{ bucket: 1700000000, requests: 3, errors: 1, latency_ms: 10, input_tokens: null, output_tokens: null, cost_micros: null }],
      { input_tokens: null, output_tokens: null, cost_micros: null },
    ))
    renderPage()
    await screen.findByText('1 failed')
    // The KPI card and the row head both say unmeasured; neither invents a zero.
    expect(document.querySelector('[data-stat="cost"]')?.textContent).toBe('—')
    expect([...document.querySelectorAll('.chart-spark')].map((row) => row.textContent)).toEqual(['Total tokens—', 'Cost—'])
    // Nothing measured means nothing drawn: no line, and no dot on the baseline.
    expect(paths('path.chart-spark-line')).toEqual(['', ''])
    expect(document.querySelectorAll('circle.chart-spark-dot')).toHaveLength(0)
    expect(cells(0).slice(3)).toEqual(['—', '—', '—', '—'])
    // The request and error series are still real counts, and are still drawn.
    expect(document.querySelector('path.chart-line')?.getAttribute('d')).toMatch(/^M 0\.00 \S+ L 640\.00 \S+$/)
  })
})
