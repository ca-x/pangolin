import { describe, expect, it } from 'vitest'
import { generateClientSetup, gatewayUrls, shellQuote, powershellQuote } from './clientSetup'

describe('safe client setup', () => {
  it('preserves deployment subpaths with exactly one protocol suffix', () => {
    for (const base of ['https://gateway.example/team/', 'https://gateway.example/team/v1/', 'https://gateway.example/team/v1/v1']) {
      expect(gatewayUrls(base)).toEqual({ root: 'https://gateway.example/team', v1: 'https://gateway.example/team/v1' })
      expect(generateClientSetup({ client: 'codex', baseUrl: base, model: 'public' })).toContain('wire_api = "responses"')
      expect(generateClientSetup({ client: 'claude', baseUrl: base, model: 'public' })).toContain('https://gateway.example/team')
      expect(generateClientSetup({ client: 'gemini', baseUrl: base, model: 'public' })).not.toContain('/v1')
    }
  })
  it.each(['ftp://example.com', 'https://u:p@example.com', 'https://example.com/?token=raw', 'https://example.com/#fragment', 'https://example.com/\npath'])('rejects unsafe base %s', (baseUrl) => {
    expect(() => gatewayUrls(baseUrl)).toThrow()
  })
  it('quotes shell and PowerShell data literally and uses safe SDK literals', () => {
    const value = 'a\'"\n$()`x`\\b'
    expect(shellQuote(value)).toBe("'a'\"'\"'\"\n$()`x`\\b'")
    expect(powershellQuote(value)).toBe("'a''\"\n$()`x`\\b'")
    for (const client of ['python', 'node', 'codex'] as const) {
      const output = generateClientSetup({ client, baseUrl: 'https://example.com', model: value, token: value })
      expect(output).toContain(JSON.stringify(value))
    }
  })
  it('uses environment credentials for existing keys and one-time credentials only when supplied', () => {
    for (const client of ['codex', 'claude', 'gemini', 'curl', 'python', 'node'] as const) {
      const options = { client, baseUrl: 'https://example.com', model: 'public' }
      expect(generateClientSetup(options)).toContain('PANGOLIN_API_KEY')
      expect(generateClientSetup({ ...options, token: 'one-time-secret' })).toContain('one-time-secret')
    }
    expect(generateClientSetup({ client: 'claude', baseUrl: 'https://example.com/v1', model: 'public' })).toContain('"env"')
    expect(generateClientSetup({ client: 'curl', baseUrl: 'https://example.com', model: 'public', shell: 'powershell' })).toContain('Invoke-RestMethod')
  })
})

describe('serialized client configuration', () => {
  it('round-trips quoted model/token data in Claude JSON and Codex TOML string values', () => {
    const model = 'model"\\\n$`'
    const token = 'token"\\\n$`'
    const claude = generateClientSetup({ client: 'claude', baseUrl: 'https://example.com/team/v1', model, token })
    const document = JSON.parse(claude.slice(claude.indexOf('\n{') + 1))
    expect(document).toEqual({ env: { ANTHROPIC_BASE_URL: 'https://example.com/team', ANTHROPIC_MODEL: model, ANTHROPIC_AUTH_TOKEN: token } })
    const codex = generateClientSetup({ client: 'codex', baseUrl: 'https://example.com/team', model, token, effort: 'high' })
    expect(JSON.parse(codex.match(/^model = (.+)$/m)![1])).toBe(model)
    expect(JSON.parse(codex.match(/^experimental_bearer_token = (.+)$/m)![1])).toBe(token)
    expect(codex).toContain('[model_providers.pangolin]')
    expect(codex).toContain('model_reasoning_effort = "high"')
    expect(codex).toContain('base_url = "https://example.com/team/v1"')
  })
  it('omits invented reasoning configuration unless explicitly selected', () => {
    expect(generateClientSetup({ client: 'python', baseUrl: 'https://example.com', model: 'public' })).not.toContain('reasoning=')
    expect(generateClientSetup({ client: 'python', baseUrl: 'https://example.com', model: 'public', effort: 'high' })).toContain('reasoning={"effort": "high"}')
  })
})
