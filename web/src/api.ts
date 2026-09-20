export type Branding = { instance_name: string; branding_name: string; favicon_url: string; onboarding_complete: boolean }
export type Bootstrap = { initialized: boolean; authenticated: boolean; user: User | null; capture_payloads: boolean; branding: Branding }
export type User = { id: string; email: string; role: string; language: string; theme: string; created_at: number }
export type Provider = { id: string; name: string; kind: string; base_url: string; enabled: boolean; created_at: number; updated_at: number }
export type Model = { id: string; provider_id: string; provider_name: string; public_name: string; upstream_name: string; capabilities: string; input_price_micros: number; output_price_micros: number; priority: number; enabled: boolean; created_at: number }
export type ApiKey = { id: string; name: string; key_prefix: string; scopes: string; budget_micros: number | null; spent_micros: number; enabled: boolean; last_used_at: number | null; created_at: number }
export type SummaryPoint = { bucket: number; requests: number; errors: number; latency_ms: number }
export type Summary = { requests: number; errors: number; error_rate: number | null; p95_latency_ms: number | null; input_tokens: number; output_tokens: number; cost_micros: number; series: SummaryPoint[] }
export type RequestItem = { request_id: string; started_at: number; endpoint: string; provider: string | null; requested_model: string | null; resolved_model: string | null; status_code: number; latency_ms: number; input_tokens: number; output_tokens: number; cost_micros: number }
export type RequestDetail = RequestItem & { trace_id: string; finished_at: number; api_key_id: string | null; error_kind: string | null; ttft_ms: number | null; cached_tokens: number; payload_captured: boolean; request_json: string | null; response_json: string | null }
export type Project = { id: string; name: string; slug: string; owner_user_id: string | null; is_default: boolean; enabled: boolean }
export type Paged<T = Record<string, unknown>> = { data: T[]; total?: number; version?: string }
export type Document = Record<string, unknown> & { id: string }

export class ApiFailure extends Error {
  constructor(public status: number, message: string) { super(message) }
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
    const payload = await response.json().catch(() => null) as { error?: { message?: string } } | null
    throw new ApiFailure(response.status, payload?.error?.message || response.statusText)
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}
