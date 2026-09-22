export type Branding = { instance_name: string; branding_name: string; favicon_url: string; onboarding_complete: boolean }
/** Provenance of the running binary, reported by `/api/v1/bootstrap` and `/api/v1/version`. */
export type BuildInfo = { version: string; commit: string; built_at: string; target: string; profile: string }
/**
 * `observability_available` is the backend's own health verdict on the DuckDB
 * projection. It is optional here only because older binaries do not report it;
 * `undefined` means "unknown", never "degraded".
 */
export type Bootstrap = { initialized: boolean; authenticated: boolean; user: User | null; capture_payloads: boolean; observability_available?: boolean; branding: Branding; build?: BuildInfo }
/** Site request-logging policy. `off` (or a disabled policy) means no request event is ever written. */
export type RequestLoggingPolicy = { version: number; enabled: boolean; default_level: string; key_override_enabled: boolean; key_disable_allowed: boolean; live_preview_enabled?: boolean }
export type User = { id: string; email: string; role: string; language: string; theme: string; created_at: number }
export type Provider = { id: string; name: string; kind: string; base_url: string; enabled: boolean; created_at: number; updated_at: number }
export type Model = { id: string; provider_id: string; provider_name: string; public_name: string; upstream_name: string; capabilities: string; input_price_micros: number; output_price_micros: number; priority: number; enabled: boolean; created_at: number }
export type ApiKey = { id: string; name: string; key_prefix: string; scopes: string; budget_micros: number | null; spent_micros: number; enabled: boolean; last_used_at: number | null; created_at: number }
/**
 * One bucket of the overview's trend. `input_tokens`, `output_tokens` and
 * `cost_micros` are `null` when the bucket settled no measured usage at all — an
 * unmeasured bucket is a gap in the series and prints `—`, while a measured zero
 * is a plotted zero. `latency_ms` is the bucket's average.
 */
export type SummaryPoint = { bucket: number; requests: number; errors: number; latency_ms: number; input_tokens: number | null; output_tokens: number | null; cost_micros: number | null }
/**
 * The overview's totals for one window. `input_tokens`, `output_tokens` and
 * `cost_micros` are `null` when the window measured no usage: a window of
 * unmeasured failures has no total, and `0` would be a measurement nobody made.
 * `requests`, `errors`, `error_rate` and `p95_latency_ms` are unchanged.
 */
export type Summary = { requests: number; errors: number; error_rate: number | null; p95_latency_ms: number | null; input_tokens: number | null; output_tokens: number | null; cost_micros: number | null; series: SummaryPoint[] }
/**
 * One row of the request log. `internal_id` is Pangolin's own request UUID: it is
 * unique, so it is the row key and the identity the detail is opened by. `request_id`
 * is the caller-supplied `x-request-id`; it is kept for correlation and must never be
 * used as an identity — a client can repeat it, and another project's row can carry
 * the same one.
 *
 * `status_code` is the upstream status the gateway actually observed, or `null`
 * when there was none to observe: a locally answered request, a failure before the
 * provider responded, or a row recorded before the status was persisted. `null`
 * renders as `—`; `0` is not an HTTP status and is never sent.
 *
 * The token facts are the settled usage counts. `cached_tokens` is the cache-read
 * count, `cache_write_tokens` and `reasoning_tokens` are what the provider reported
 * even when no price charges for them. `ttft_ms` is `null` when no first byte was
 * measured — a non-stream response has none to measure — and `stream` is the
 * request's own admission decision: `true`, `false`, or `null` when nothing
 * recorded it, which is not the same fact as `false`.
 */
export type RequestItem = { internal_id: string; request_id: string; started_at: number; endpoint: string; provider: string | null; requested_model: string | null; resolved_model: string | null; status_code: number | null; error_kind: string | null; latency_ms: number; ttft_ms: number | null; input_tokens: number; output_tokens: number; cached_tokens: number | null; cache_write_tokens: number | null; reasoning_tokens: number | null; stream: boolean | null; cost_micros: number; api_key_id: string | null }
/**
 * One row of `/analytics`: the aggregate of a single value of the selected
 * dimension. `dimension` is an opaque id for every entity facet, and `latency_ms`
 * and `ttft_ms` are averages the projection may not be able to compute.
 */
export type AnalyticsRow = { dimension: string | null; requests: number; attempts: number; errors: number; usage_measured?: boolean; input_tokens: number; output_tokens: number; cache_hit_tokens: number; cache_savings_micros: number; cost_micros: number; latency_ms: number | null; ttft_ms: number | null; tokens_per_second?: number | null }
export type LiveRequest = { model: string; channel_id: string; api_key_id: string; started_at: number }
/**
 * The projection's detail document for one request: the same facts as a list row plus
 * the record-system fields. Its own identity is `id` — the internal UUID the list row
 * exposes as `internal_id` — while `request_id` stays the caller's `x-request-id`.
 * `error_kind` is why the request failed; the gateway normalizes the status code, so
 * the kind is the actionable half.
 */
export type RequestDetail = Omit<RequestItem, 'internal_id'> & { id: string; trace_id: string; finished_at: number; api_key_id: string | null; source_ip: string | null; error_kind: string | null; payload_captured: boolean; request_json: string | null; response_json: string | null }
export type Project = { id: string; name: string; slug: string; owner_user_id: string | null; is_default: boolean; enabled: boolean }
export type Paged<T = Record<string, unknown>> = { data: T[]; total?: number; version?: string }
export type Document = Record<string, unknown> & { id: string }

export class ApiFailure extends Error {
  constructor(public status: number, message: string, public code?: string) { super(message) }
}
export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const method = (init?.method || 'GET').toUpperCase()
  const mutation = !['GET', 'HEAD', 'OPTIONS'].includes(method)
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
    headers: { 'Content-Type': 'application/json', ...(mutation ? { 'X-Pangolin-CSRF': '1' } : {}), ...init?.headers },
  })
  if (!response.ok) {
    // The code lives in `error.type` on the OpenAI envelope and in `error.code` on
    // Anthropic's; Gemini's `error.code` is the numeric status, so only a string is
    // taken from that field.
    const payload = await response.json().catch(() => null) as { error?: { type?: string; code?: string | number; message?: string } } | null
    const code = payload?.error?.type || (typeof payload?.error?.code === 'string' ? payload.error.code : undefined)
    throw new ApiFailure(response.status, payload?.error?.message || response.statusText, code)
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}
