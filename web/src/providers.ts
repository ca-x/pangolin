/**
 * Channel providers offered by the channel form's provider picker.
 *
 * The list mirrors two Pangolin sources of truth instead of inventing values:
 * `src/providers/mod.rs::default_base` (the backend's default endpoint per
 * adapter kind) and the built-in catalog's `logo_key`. Icons are resolved by
 * `ProviderIcon` against the bundled icon libraries, so no brand asset is
 * copied into the console.
 */
export type ChannelProvider = {
  /** Adapter kind submitted as the channel `kind`. */
  kind: string
  /** i18n key of the provider name shown in the picker. */
  labelKey: string
  /** Built-in catalog `logo_key`, resolved against the bundled icon map. */
  logoKey: string
  /** Default endpoint for the kind, or null when the provider needs a custom one. */
  defaultBaseUrl: string | null
}

export const CHANNEL_PROVIDERS: readonly ChannelProvider[] = [
  { kind: 'openai', labelKey: 'providerLabelOpenai', logoKey: 'lobehub:OpenAI', defaultBaseUrl: 'https://api.openai.com/v1' },
  { kind: 'openai_compatible', labelKey: 'providerLabelOpenaiCompatible', logoKey: 'lobehub:OpenAI', defaultBaseUrl: null },
  { kind: 'anthropic', labelKey: 'providerLabelAnthropic', logoKey: 'lobehub:Anthropic', defaultBaseUrl: 'https://api.anthropic.com' },
  { kind: 'gemini', labelKey: 'providerLabelGemini', logoKey: 'lobehub:Gemini', defaultBaseUrl: 'https://generativelanguage.googleapis.com' },
  { kind: 'azure', labelKey: 'providerLabelAzure', logoKey: 'lobehub:Azure', defaultBaseUrl: null },
  { kind: 'bedrock', labelKey: 'providerLabelBedrock', logoKey: 'lobehub:Aws', defaultBaseUrl: null },
  { kind: 'vertex', labelKey: 'providerLabelVertex', logoKey: 'lobehub:VertexAI', defaultBaseUrl: null },
  { kind: 'gcp', labelKey: 'providerLabelGcp', logoKey: 'lobehub:GoogleCloud', defaultBaseUrl: null },
  { kind: 'openrouter', labelKey: 'providerLabelOpenrouter', logoKey: 'lobehub:OpenRouter', defaultBaseUrl: 'https://openrouter.ai/api/v1' },
  { kind: 'deepseek', labelKey: 'providerLabelDeepseek', logoKey: 'lobehub:DeepSeek', defaultBaseUrl: 'https://api.deepseek.com/v1' },
  { kind: 'moonshot', labelKey: 'providerLabelMoonshot', logoKey: 'lobehub:Moonshot', defaultBaseUrl: 'https://api.moonshot.cn/v1' },
  { kind: 'zhipu', labelKey: 'providerLabelZhipu', logoKey: 'lobehub:Zhipu', defaultBaseUrl: 'https://open.bigmodel.cn/api/paas/v4' },
  { kind: 'doubao', labelKey: 'providerLabelDoubao', logoKey: 'lobehub:Doubao', defaultBaseUrl: 'https://ark.cn-beijing.volces.com/api/v3' },
  { kind: 'xai', labelKey: 'providerLabelXai', logoKey: 'lobehub:XAI', defaultBaseUrl: 'https://api.x.ai/v1' },
  { kind: 'groq', labelKey: 'providerLabelGroq', logoKey: 'lobehub:Groq', defaultBaseUrl: 'https://api.groq.com/openai/v1' },
  { kind: 'ollama', labelKey: 'providerLabelOllama', logoKey: 'lobehub:Ollama', defaultBaseUrl: 'http://127.0.0.1:11434/v1' },
  { kind: 'nanogpt', labelKey: 'providerLabelNanogpt', logoKey: 'lobehub:NanoGPT', defaultBaseUrl: 'https://nano-gpt.com/api/v1' },
  { kind: 'jina', labelKey: 'providerLabelJina', logoKey: 'lobehub:Jina', defaultBaseUrl: 'https://api.jina.ai/v1' },
]
