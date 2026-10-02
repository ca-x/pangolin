/** Client setup is pure and never reads or edits client files or persists tokens. */
export type SetupClient = 'codex' | 'claude' | 'gemini' | 'curl' | 'python' | 'node'
export type SetupOptions = { client: SetupClient; baseUrl: string; model: string; token?: string; effort?: string; shell?: 'posix' | 'powershell' }
export const SETUP_CLIENTS: SetupClient[] = ['codex', 'claude', 'gemini', 'curl', 'python', 'node']
export const CLIENT_LABELS: Record<SetupClient, string> = { codex: 'Codex', claude: 'Claude Code', gemini: 'Gemini CLI', curl: 'curl', python: 'Python', node: 'Node' }
export function clientProtocol(client: SetupClient) {
  if (client === 'claude') return { endpoint: '/v1/messages', stream: true }
  if (client === 'gemini') return { endpoint: '/v1beta/models:streamGenerateContent', stream: true }
  return { endpoint: '/v1/responses', stream: client === 'codex' }
}
export function gatewayUrls(baseUrl: string): { root: string; v1: string } {
  if (baseUrl.length > 2048 || /[\u0000-\u0020\u007f]/.test(baseUrl)) throw new Error('invalid_base_url')
  const url = new URL(baseUrl)
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) throw new Error('invalid_base_url')
  const path = url.pathname.replace(/\/+$/, '').replace(/(?:\/v1)+$/, '')
  const root = `${url.origin}${path}`
  return { root, v1: `${root}/v1` }
}
export function defaultGatewayBase(): string {
  // The SPA is mounted beneath the deployment base; do not infer API roots from
  // console routes such as /access. Operators can confirm proxy rewrites here.
  return new URL(import.meta.env.BASE_URL, window.location.origin).href
}
export const shellQuote = (value: string) => `'${value.replaceAll("'", "'\"'\"'")}'`
export const powershellQuote = (value: string) => `'${value.replaceAll("'", "''")}'`
const literal = (value: string) => JSON.stringify(value).replaceAll(String.fromCharCode(127), "\\u007f")

export function generateClientSetup({ client, baseUrl, model, token, effort, shell = 'posix' }: SetupOptions): string {
  const { root, v1 } = gatewayUrls(baseUrl)
  const quote = shell === 'powershell' ? powershellQuote : shellQuote
  const setEnv = (key: string, value: string) => shell === 'powershell' ? `$env:${key} = ${quote(value)}` : `export ${key}=${quote(value)}`
  const envRef = shell === 'powershell' ? '$env:PANGOLIN_API_KEY' : '"$PANGOLIN_API_KEY"'
  const tokenEnv = token === undefined ? [] : [setEnv('PANGOLIN_API_KEY', token)]
  const aliasEnv = (name: string) => shell === 'powershell' ? `$env:${name} = $env:PANGOLIN_API_KEY` : `export ${name}="$PANGOLIN_API_KEY"`
  switch (client) {
    case 'codex':
      return [`model = ${literal(model)}`, 'model_provider = "pangolin"', ...(effort ? [`model_reasoning_effort = ${literal(effort)}`] : []), '', '[model_providers.pangolin]', 'name = "Pangolin"', `base_url = ${literal(v1)}`, 'wire_api = "responses"', token === undefined ? 'env_key = "PANGOLIN_API_KEY"' : `experimental_bearer_token = ${literal(token)}`].join('\n')
    case 'claude':
      // Claude appends /v1/messages to its root. Existing tokens are resolved by
      // the shell, never a fake ${ENV} string in a JSON configuration.
      return [...tokenEnv, aliasEnv('ANTHROPIC_AUTH_TOKEN'), setEnv('ANTHROPIC_BASE_URL', root), setEnv('ANTHROPIC_MODEL', model), '', JSON.stringify({ env: { ANTHROPIC_BASE_URL: root, ANTHROPIC_MODEL: model, ...(token === undefined ? {} : { ANTHROPIC_AUTH_TOKEN: token }) } }, null, 2)].join('\n')
    case 'gemini':
      return [...tokenEnv, aliasEnv('GEMINI_API_KEY'), setEnv('GOOGLE_GEMINI_BASE_URL', root), `gemini --model ${quote(model)}`].join('\n')
    case 'curl': {
      const body = JSON.stringify({ model, input: 'Hello', stream: false, ...(effort ? { reasoning: { effort } } : {}) })
      if (shell === 'powershell') return [...tokenEnv, `$headers = @{ Authorization = ("Bearer " + ${envRef}); 'Content-Type' = 'application/json' }`, `Invoke-RestMethod -Method Post -Uri ${quote(`${v1}/responses`)} -Headers $headers -Body ${quote(body)}`].join('\n')
      return [...tokenEnv, `curl ${quote(`${v1}/responses`)} \\\n  -H "Authorization: Bearer $PANGOLIN_API_KEY" \\\n  -H 'Content-Type: application/json' \\\n  --data-raw ${quote(body)}`].join('\n')
    }
    case 'python':
      return `import os\nfrom openai import OpenAI\n\nclient = OpenAI(base_url=${literal(v1)}, api_key=${token === undefined ? 'os.environ["PANGOLIN_API_KEY"]' : literal(token)})\nresponse = client.responses.create(model=${literal(model)}, input="Hello", stream=False${effort ? `, reasoning={"effort": ${literal(effort)}}` : ''})\nprint(response.output_text)`
    case 'node':
      return `import OpenAI from "openai";\n\nconst client = new OpenAI({ baseURL: ${literal(v1)}, apiKey: ${token === undefined ? 'process.env.PANGOLIN_API_KEY' : literal(token)} });\nconst response = await client.responses.create({ model: ${literal(model)}, input: "Hello", stream: false${effort ? `, reasoning: { effort: ${literal(effort)} }` : ''} });\nconsole.log(response.output_text);`
  }
}
