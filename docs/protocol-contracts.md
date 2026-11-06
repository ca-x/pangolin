# Gateway protocol contracts

All gateway generation requests use the same project/API-key access, profile mapping, candidate selection, prompt protection, admission, retry, circuit, request/trace identifiers and observation boundary. Model discovery uses that boundary's authorized candidate query. Native JSON request fields and upstream response bodies are preserved, apart from routing model substitution and explicit policy overrides.

## Endpoints

| Public route | Transport / behavior |
| --- | --- |
| `/v1/chat/completions`, `/v1/completions` | JSON and SSE, `[DONE]` terminal |
| `/v1/responses` | JSON/SSE; GET upgrades to Responses WebSocket |
| `/v1/responses/compact` | Native compaction request/response, scoped prior-response restoration |
| `/v1/models`, `/v1/models/{model}` | Project/key/profile-aware list and retrieve, including slash-containing names |
| `/v1/embeddings`, `/v1/moderations`, `/v1/alpha/search` | Native JSON; omitted moderation model defaults to `omni-moderation-latest` |
| `/v1/images/generations`, `/v1/images/edits` | JSON generations; multipart edits preserve file bytes, filenames, MIME types and text fields |
| `/v1/videos`, `/v1/videos/{id}` | JSON or multipart create; GET/DELETE pinned to the creating channel, credential and model |
| `/v1/audio/speech` | Binary response and Content-Type preserved |
| `/v1/audio/transcriptions`, `/v1/audio/translations` | Multipart audio request; JSON/text response preserved |
| `/v1/messages`, `/anthropic/v1/messages` | Native Anthropic JSON/SSE, version/beta headers, `message_stop` terminal |
| `/anthropic/v1/models` | Anthropic model-list envelope restricted to routable Messages models |
| `/v1/rerank`, `/jina/v1/rerank`, `/jina/v1/embeddings` | Native Jina-compatible JSON |
| `/v1beta/models/{model}:generateContent`, `:streamGenerateContent` | Native Gemini JSON/SSE; Bearer, `x-goog-api-key`, or URL-encoded `key` query authentication |
| `/gemini/{version}/models/{model}:generateContent`, `:streamGenerateContent` | Gemini namespace; `v1`, `v1beta`, `v1alpha` versions |
| `/v1beta/models`, `/gemini/{version}/models` | Gemini model envelope restricted to routable generation models |
| `/doubao/v3/contents/generations/tasks`, `/{id}` | Native JSON create and key-owned GET/DELETE; upstream `/api/v3/contents/generations/tasks` |
| `/aisdk/v1/chat/completions` | AI SDK text messages/parts; `x-vercel-ai-ui-message-stream: v1` selects UI SSE, otherwise legacy data stream |

Model capabilities are explicit: `chat`, `completions`, `responses`, `messages`, `embeddings`, `moderations`, `search`, `images`, `videos`, `speech`, `transcriptions`, `translations`, `rerank`, `gemini`. A canonical endpoint path can also be used as a capability. Alias routes share their canonical endpoint policy; for example Jina embeddings use `/v1/embeddings` access policy.

Responses WebSocket supports a long-lived `response.create` loop with a separate processing timeout per event. Optional `stream_id` values have independent FIFO queues, bounded to 16 streams and 8 queued requests per stream. The stream ID is returned on events and is never sent upstream. `stream` is forced on and `background` is a transport-only field. Completed responses use the same encrypted durable session store as HTTP/SSE. The optional `generate` control is explicitly unsupported. Errors do not close other streams. Disconnect cancels owned attempts.

Local internal failures share one sanitized error projection across HTTP, native aliases and WebSocket. Gemini errors use canonical statuses such as `UNAUTHENTICATED`, `PERMISSION_DENIED`, `RESOURCE_EXHAUSTED`, `INTERNAL` and `UNAVAILABLE`; Anthropic errors use the corresponding authentication, permission, rate-limit, request and API error types. Extractor/JSON failures are also wrapped in the requested protocol. An explicitly configured upstream pass-through policy retains its original error body. Request tracing records method and path only; query parameters, including Gemini `key`, are excluded.

WebSocket reads and writes run independently so a slow consumer cannot prevent disconnect cancellation; output buffering and write duration are bounded. Gemini streaming tracks terminal candidates across events and waits for every requested candidate.

Task ownership is durable in SQLite. A task cannot be retrieved or deleted by a different API key, even in the same project. Disabling the original channel, key, credential or model prevents further access through that route. This layer preserves provider asset URLs; storage/retention policy belongs to the operations layer.

## Providers and authentication

Provider kinds: `openai`, `openai_compatible`, `anthropic`, `gemini`, `azure`, `bedrock`, `vertex`, `gcp`, `openrouter`, `deepseek`, `moonshot`, `zhipu`, `doubao`, `xai`, `groq`, `ollama`, `nanogpt`, `jina`.

Named providers have default API bases when `base_url` is omitted; Azure/Bedrock/Vertex and generic compatible endpoints require an explicit base. Custom bases always win. Ollama may omit its credential. All other credentials are encrypted channel data. Incoming client credentials cannot replace channel credentials.

| Family | Channel credential |
| --- | --- |
| OpenAI-compatible | Bearer API key |
| Anthropic | API key or `sk-ant-oat…` OAuth bearer on native Messages |
| Gemini | API key via `x-goog-api-key` |
| Azure | API key, explicit `azure_ad_token`, or explicitly configured client-secret, workload or managed identity; optional `api_version`, `azure_scope`, HTTPS `azure_authority_host` |
| Bedrock | JSON with `region`, `access_key_id`, `secret_access_key`, optional `session_token`; explicit `profile` and optional `role_arn` use the AWS Rust SDK |
| Vertex/GCP | JSON with `vertex_project`, `vertex_location` and `vertex_credentials` service-account JSON; `access_token` supports externally managed tokens |

Vertex also accepts the existing `vertex_ai_project`, `vertex_ai_location` and
`vertex_ai_credentials` aliases. Nonempty canonical values take precedence;
null/blank values are unset, as is an empty service-account object. Malformed
canonical types fail rather than selecting an alias. Service-account credentials
may be a JSON object, a JSON-encoded string or an explicitly configured regular
file path (64 KiB maximum). File content, not its path, identifies the cached
account, so replacing a file does not reuse stale credentials. A valid external
`access_token` can obtain a missing project from that explicit account without
exchanging the token. An explicitly malformed `access_token` never falls back
to service-account credentials. No environment or ADC source is selected.

Azure uses deployment URLs and API versions, with `/openai/v1/responses` for Responses. Bedrock uses checked Converse transformation and signs the exact serialized JSON with `aws-sigv4`. Azure tokens use `azure_identity`; Vertex generates project/location/publisher URLs and uses `gcp_auth` for service-account token refresh. Credentials must be explicitly configured; malformed explicit tokens fail rather than falling back to another source. No real cloud credentials are used by the test suite.

### Azure Entra identity selection and migration

API keys retain their separate explicit authentication path. For Entra token
authentication, a valid `azure_ad_token` is used directly after validating any
identity selector. Without that token, `azure_credential` chooses exactly one
SDK-backed identity:

- `ClientSecretCredential` requires explicit `tenant_id`, `client_id` and
  `client_secret`. An omitted/null/empty selector uses this mode, without
  discovering credentials from the process environment.
- `WorkloadIdentityCredential` requires explicit `tenant_id`, `client_id` and
  `azure_federated_token_file`. The regular token file is read off the async
  executor, bounded to 64 KiB and reloaded when the SDK needs a new assertion.
- `ManagedIdentityCredential` uses the configured `client_id`, or explicitly
  selects the system-assigned identity when it is omitted/null/empty. Only this
  selected mode consults Azure's managed-identity platform metadata and endpoint
  settings; it does not try developer CLI logins or client-secret environment
  variables.

**Migration:** `DefaultAzureCredential` and `DeploymentIdentityCredential`
chains are intentionally rejected, including when an Entra profile also
contains a direct token or client secret. Remove the old chain selector for
direct-token/client-secret profiles, or name the single workload/managed mode
explicitly. A workload/managed selector is authoritative over an additional
client secret; failure never switches to another identity. Enabling
`enable_azure_ad_token_refresh` alone cannot trigger credential discovery.
Unknown selectors and malformed selected credentials fail closed. This is an
explicit security tightening of the former automatic-chain behavior, not a
claim of complete legacy Azure-auth parity.

## Explicit transformation limits

Native protocols preserve provider extension fields. Cross-protocol transformations are narrower and reject unknown fields instead of dropping them:

- OpenAI chat → Anthropic: independently checked text/system/tool-call/tool-result bridge; nonstreaming only. A string `stop` is normalized to the native `stop_sequences` array for text and tool requests alike; native Messages remains unchanged.
- OpenAI chat ↔ Gemini: text, system and supported sampling/token-limit fields; nonstreaming only. Use native Gemini for tools, multimodal inputs and streaming.
- Anthropic Messages → OpenAI chat: nonstreaming text/system and function-tool definitions, calls and text-only results with preserved IDs/JSON arguments; OpenAI tool-call responses return Anthropic `tool_use` blocks. Unsupported block ordering, error/media/server-tool results, malformed arguments and unrepresentable controls fail explicitly rather than silently losing data. Use native Anthropic for streaming and richer content.
- OpenAI embeddings → Gemini: text/string arrays and dimensions; token-array inputs are rejected.
- OpenAI chat → Bedrock: independently checked text-only Converse surface; unsupported tools/streaming are rejected. Normalized gross input includes measured input/cache-read/cache-write components once; absent components remain unmeasured, not zero.
- AI SDK: text messages/parts and both stream framing formats; nontext/tool/reasoning parts fail explicitly.

Gemini control objects use the canonical camelCase names (`generationConfig`, `systemInstruction`, `toolConfig`); alternate snake_case control shapes fail explicitly. Streaming is limited to conversational protocols; media streaming variants fail before contacting a provider. TPM policies also decline remote cached-context references and multimodal embedding inputs without a provider estimator.

Multipart requests are limited to 64 MiB and 128 fields; duplicate text field names fail explicitly, while repeated file fields retain each file. Buffered upstream responses retain the existing 16 MiB bound. Raw upstream streaming has a 1 MiB frame bound and never retries after downstream commitment. Internal WebSocket/AI SDK reparsing permits 2 MiB to accommodate the gateway's added `data: ` spacing without relaxing the upstream limit.

Supported conversational streams use conservative hard-budget reservations and final source-usage settlement. Hard-budget media requires configured flat/unit prices and trustworthy bounded image/video/speech quantities; uploaded media with unknown duration requires flat pricing. TPM policies still reject unsupported nontext quantities. Nontext TPM requests fail explicitly. Text keeps conservative byte-plus-output reservations; an operator-provided `PANGOLIN_TOKENIZER_CL100K` rank file enables `tiktoken-rs` counting for recognized GPT-4/GPT-3.5 text-chat shapes. Other models, tool and multimodal shapes retain the conservative reservation. This optional local estimate is not reported usage and is not a universal hard context-window guarantee. Token counts use a bounded memory cache containing hashes/counts, not prompt text. Redis token-count caching is not enabled.

Analytics displays independently measured input/output/cache-read/cache-write/reasoning subtotals. Total Tokens sums input plus output only within attempts where both are measured; coverage shows those attempts over all attempts. It never adds independently aggregated partial counters or counts cache/reasoning subsets twice. Unknown is `—`; a measured zero stays zero.

## Upstream reuse and test evidence

`Cargo.toml` and `Cargo.lock` pin MIT `arcships/aimux` at `4c0b40c54d689439937155d2c36720a4b3c7454d`. Pangolin implements its `Fetch` interface with the configured reqwest client, consumes `FetchResponse` and the bounded aimux SSE parser, and resolves explicit bearer credentials through `ProviderHeaders`. Native bytes and fields bypass normalized generation APIs. Pangolin still owns proxy/DNS policy, protocol transforms, authorization, retry/admission and terminal accounting. No LiteLLM, Python bridge or legacy host dependency remains. Aimux's wider provider catalog is not a claim of tested Pangolin integration.

The independent fixture table lives at `src/api/gateway/tests/fixtures/axonhub_protocols.json`, with source test/route attribution per row. Composed integration tests cover raw field preservation, upstream model/auth selection, multipart bytes, binary speech, native Anthropic/Gemini stream terminals, task ownership, model formats, Responses WebSocket continuation and AI SDK framing. Provider contract tests cover compatible presets, Azure deployment/version/auth, signed Bedrock requests and Vertex project/auth URLs. Task 3's complete retry/admission/session regression suite remains binding.

These providers are **contract-tested**, not live-tested. Realtime remains AxonHub's upstream Todo.

The versioned offline catalog, online subscriptions, signatures, local overrides and setup metadata are documented in [catalog.md](catalog.md). Catalog breadth does not imply that every listed vendor has a live adapter.

## Source measurements and structured protection

Source protocol fields determine usage before generic response conversion, and native/SSE accounting uses the same source-bound counter projection. Foreign protocol containers and compatibility zeros are retained where native forwarding requires them, but cannot become measured counters. Gemini total/thought semantics, OpenAI compatible detail aliases and Anthropic split-cache/input reports use checked presence/arithmetic. Original response/event values still control forwarding, terminals, IDs and durable sessions. Quantity availability is independently exposed in `usage_measurement`; known cost never proves an unreported counter was zero.

Structured protection applies to supported conversational/tool text across Chat, Responses, Messages and Gemini. JSON keys, typed values, IDs, media and recognized native opaque continuation remain intact; ordinary user/tool signature-named strings are inspectable. Invalid replay carriers and duplicate tool JSON keys reject explicitly. This is structure recognition, not signature verification or OCR. The full-request preview applies the same traversal with endpoint/body context only; its bounds, transient sample handling and attempt diagnostics are described in [operations.md](operations.md).
