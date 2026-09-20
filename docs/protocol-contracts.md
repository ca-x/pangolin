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

Task ownership is durable in SQLite. A task cannot be retrieved or deleted by a different API key, even in the same project. Disabling the original channel, key, credential or model prevents further access through that route. This layer preserves provider asset URLs; storage/retention policy belongs to the operations layer.

## Providers and authentication

Provider kinds: `openai`, `openai_compatible`, `anthropic`, `gemini`, `azure`, `bedrock`, `vertex`, `gcp`, `openrouter`, `deepseek`, `moonshot`, `zhipu`, `doubao`, `xai`, `groq`, `ollama`, `nanogpt`, `jina`.

Named providers have default API bases when `base_url` is omitted; Azure/Bedrock/Vertex and generic compatible endpoints require an explicit base. Custom bases always win. Ollama may omit its credential. All other credentials are encrypted channel data. Incoming client credentials cannot replace channel credentials.

| Family | Channel credential |
| --- | --- |
| OpenAI-compatible | Bearer API key |
| Anthropic | API key or `sk-ant-oat…` OAuth bearer on native Messages |
| Gemini | API key via `x-goog-api-key` |
| Azure | API key, or JSON containing `azure_ad_token`, or LiteLLM Azure tenant/client/credential parameters; optional `api_version` |
| Bedrock | JSON with `region`, `access_key_id`, `secret_access_key`, optional `session_token`; role/profile resolution also delegates to LiteLLM AWS auth |
| Vertex/GCP | JSON with `vertex_project`, `vertex_location` and `vertex_credentials` service-account JSON; `access_token` supports externally managed tokens |

Azure uses deployment URLs and API versions, with `/openai/v1/responses` for Responses. Bedrock uses checked Converse transformation and signs the exact serialized JSON with AWS SigV4. Vertex generates project/location/publisher URLs and delegates credential/token refresh to LiteLLM GCP auth. No real cloud credentials are used by the test suite.

## Explicit transformation limits

Native protocols preserve provider extension fields. Cross-protocol transformations are narrower and reject unknown fields instead of dropping them:

- OpenAI chat → Anthropic: text/system/tool-call/tool-result bridge, with LiteLLM's checked text transformation when applicable; nonstreaming only.
- OpenAI chat ↔ Gemini and Anthropic Messages → OpenAI chat: text, system and supported sampling/token-limit fields; nonstreaming only. Use native endpoints for tools, multimodal inputs and streaming.
- OpenAI embeddings → Gemini: text/string arrays and dimensions; token-array inputs are rejected.
- OpenAI chat → Bedrock: the pinned LiteLLM checked text Converse surface; unsupported tools/streaming are rejected.
- AI SDK: text messages/parts and both stream framing formats; nontext/tool/reasoning parts fail explicitly.

Gemini control objects use the canonical camelCase names (`generationConfig`, `systemInstruction`, `toolConfig`); alternate snake_case control shapes fail explicitly. Streaming is limited to conversational protocols; media streaming variants fail before contacting a provider. TPM policies also decline remote cached-context references and multimodal embedding inputs without a provider estimator.

Multipart requests are limited to 64 MiB and 128 fields; duplicate text field names fail explicitly, while repeated file fields retain each file. Buffered upstream responses retain the existing 16 MiB bound. Streaming has a 1 MiB frame bound and never retries after downstream commitment.

Budget-limited keys/profiles still reject streaming until usage settlement is available. Media endpoints also reject budget-limited keys/profiles and TPM policies because token-only price estimates cannot reliably account for image/audio/video units. Nontext TPM requests fail explicitly. Text keeps conservative byte-plus-output reservations; an operator-provided `PANGOLIN_TOKENIZER_CL100K` rank file enables LiteLLM token counting for recognized GPT-4/GPT-3.5 text-chat shapes. Other models and shapes retain the conservative reservation. Token counts use a bounded memory cache containing hashes/counts, not prompt text. Redis caching is not enabled.

## Upstream reuse and test evidence

`Cargo.toml` and `Cargo.lock` pin the official LiteLLM repository to `8c4c394ecc82c4d6acb5eb371d8781e487894a17`. Pangolin adapters directly consume `litellm-types`, `litellm-core`, `litellm-llms`, `litellm-framing`, `litellm-auth`, AWS/Azure/GCP auth, HTTP settings, token counting and memory-cache interfaces. The transitive `litellm-host` crate contains pure Rust interfaces only. Python bridge, `host-python` and legacy callbacks are absent.

The independent fixture table lives at `src/api/gateway/tests/fixtures/axonhub_protocols.json`, with source test/route attribution per row. Composed integration tests cover raw field preservation, upstream model/auth selection, multipart bytes, binary speech, native Anthropic/Gemini stream terminals, task ownership, model formats, Responses WebSocket continuation and AI SDK framing. Provider contract tests cover compatible presets, Azure deployment/version/auth, signed Bedrock requests and Vertex project/auth URLs. Task 3's complete retry/admission/session regression suite remains binding.

These providers are **contract-tested**, not live-tested. Realtime remains AxonHub's upstream Todo.

The versioned offline catalog, online subscriptions, signatures, local overrides and setup metadata are documented in [catalog.md](catalog.md). Catalog breadth does not imply that every listed vendor has a live adapter.
