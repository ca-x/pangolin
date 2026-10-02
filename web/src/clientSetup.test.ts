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

// Contract scanner for the literal subset emitted by these snippets. PowerShell
// CharTraits.IsSingleQuote recognizes ASCII and all four U+2018..U+201B forms;
// its literal scanner consumes doubled quote delimiters as data. This does not
// execute snippets or pretend to validate arbitrary PowerShell grammar.
function powerShellLiteralContract(source: string) {
  const quotes = new Set(["'", '\u2018', '\u2019', '\u201a', '\u201b'])
  const values: string[] = []
  const statements: string[] = []
  let text = '', depth = 0
  const finish = () => { if (text.trim()) statements.push(text.trim()); text = '' }
  for (let index = 0; index < source.length; index += 1) {
    const start = source[index]
    if (quotes.has(start) || start === '"') {
      const single = quotes.has(start)
      let value = '', closed = false
      while (++index < source.length) {
        const character = source[index]
        if ((single && quotes.has(character)) || (!single && character === '"')) {
          if (single && quotes.has(source[index + 1])) { value += character; index += 1; continue }
          closed = true; break
        }
        value += character
      }
      if (!closed) throw new Error('Unterminated PowerShell string')
      values.push(value); text += '<literal>'; continue
    }
    if (start === '#') { while (index < source.length && source[index] !== '\n') index += 1; index -= 1; continue }
    if (['{', '(', '['].includes(start)) depth += 1
    if (['}', ')', ']'].includes(start)) depth -= 1
    if ((start === '\n' || start === ';') && depth === 0) finish()
    else text += start
  }
  finish()
  return { values, statements }
}

describe('PowerShell smart-quote command boundaries', () => {
  it.each(['\u2018', '\u2019', '\u201a', '\u201b'].flatMap((quote) => ['claude', 'gemini', 'curl'].map((client) => [quote, client] as const)))('keeps %s data literal in the full %s snippet', (quote, client) => {
    const model = `public${quote}; Write-Output PANGOLIN_MODEL_INJECTION; #\n$()`
    const token = `token${quote}; Write-Output PANGOLIN_TOKEN_INJECTION; #\n$()`
    const options = { client: client as 'claude' | 'gemini' | 'curl', baseUrl: 'https://example.com/team', shell: 'powershell' as const }
    const prefix = (snippet: string) => client === 'claude' ? snippet.split('\n{')[0] : snippet
    const baseline = powerShellLiteralContract(prefix(generateClientSetup({ ...options, model: 'public', token: 'token' })))
    const snippet = generateClientSetup({ ...options, model, token })
    const parsed = powerShellLiteralContract(prefix(snippet))
    expect(parsed.statements).toEqual(baseline.statements)
    expect(parsed.values).toContain(token)
    if (client === 'curl') expect(JSON.parse(parsed.values.find((value) => value.startsWith('{"model":'))!).model).toBe(model)
    else expect(parsed.values).toContain(model)
    if (client === 'claude') {
      const settings = JSON.parse(snippet.slice(snippet.indexOf('\n{') + 1))
      expect(settings.env.ANTHROPIC_MODEL).toBe(model)
      expect(settings.env.ANTHROPIC_AUTH_TOKEN).toBe(token)
    }
  })
})
