# new-api 基础 API / Token 用量核对

## 核对范围与来源

- 核对日期：2026-11-06；鲮鲤基线：`6ac95bce771216c59c1fd5ad01ece24fda90e834`。
- 本地行为参考：`/home/czyt/code/go/new-api`，commit `e03439033238722c0b8d4437a052445f3aeeed83`，AGPL-3.0。仅核对路由、可见功能及测试场景，没有复制 Go/RelayKit/前端源码或图片资源。
- 接入依赖：`arcships/aimux`，commit `4c0b40c54d689439937155d2c36720a4b3c7454d`，MIT。直接使用 Fetch/FetchResponse、显式凭证头与 SSE 接口；不使用有损 normalized generation API 接管原生透传。
- 用户明确限定：基础 API 与 Token 使用统计；不做会员、充值、钱包、支付、兑换码、商业套餐或付费订阅。模型目录的数据订阅不是商业订阅，保留原有能力。
- 本表是限定范围的行为核对，不替代或删减 `axonhub-capability-matrix.md`，也不宣称完整 new-api 或 aimux 全供应商对齐。

## 功能核对

“已有”表示当前实现存在，对应验证结果另见 `../tasks/aimux-core-api-plan.md`；没有真实云凭证，不标记 live-tested。

| 能力 | new-api 行为参考 | 鲮鲤实现 / 本次变更 |
| --- | --- | --- |
| Chat / legacy Completions / SSE | `router/relay-router.go` | 已有 `/v1/chat/completions`、`/v1/completions`；改用 aimux Fetch 边界，保留项目/Key 策略及终态统计 |
| Responses HTTP / compact / WebSocket | `router/relay-router.go` | 已有 `/v1/responses`、`/v1/responses/compact`；WebSocket 仍逐事件检查准入，不引入 Realtime 扩展 |
| 原生 Anthropic / Gemini | `router/relay-router.go` | 已有 Messages 和 Gemini generate/streamGenerateContent；原生扩展字段透传，跨协议不支持的结构明确拒绝 |
| 模型列表与详情 | `router/relay-router.go` 的 models 路由 | 已有 API-Key 可见模型、可路由性过滤、OpenAI/Anthropic/Gemini 格式与隐藏规则 |
| Embeddings / rerank / moderation | `router/relay-router.go` | 已有对应标准路由和 Jina 兼容入口，沿用统一策略和 accounting |
| 图片 / 音频 / 视频 | relay 路由与任务入口 | 保留已实现的标准图片、multipart 音频、二进制 speech、视频和已有异步任务能力；不借机扩展插件市场 |
| 渠道、模型与 Key 管理 | `router/api-router.go` | 已有启停、优先级/权重、模型映射、Key 生命周期、作用域与项目隔离；保留控制面事务审计 |
| 故障转移与限流 | relay middleware 与渠道行为 | 已有重试、熔断、并发/RPM/TPM、预算和亲和；维持首字节提交后禁止重试 |
| 提供方认证 | 渠道凭证配置行为 | aimux 显式 Bearer；Azure 显式 client-secret/workload/managed identity、AWS SDK/SigV4、gcp_auth；无隐式默认凭证回退 |
| 请求/执行明细 | `router/api-router.go` 的 log 路由 | 已有 Request/Attempt 明细、终态与原始计量可信标记；本地估算不冒充上游用量 |
| Token 分项 | log/stat 的计量行为 | 保留 input/output，补齐 analytics 的 cache-write 与 reasoning 独立实测聚合；cache-read 单独展示 |
| Total Tokens | log/stat 的汇总需求 | 本次新增同一 Attempt 的已测 input+output 配对总量与 `total_tokens_samples` 覆盖数；不把不同 Attempt 的半边统计硬相加 |
| 聚合筛选 | log/stat 的时间/模型/渠道/Token 筛选 | 沿用项目级 analytics：Key、模型、渠道、时间等维度；拒绝跨项目 Key，SQLite 是事实来源 |
| 用量交互 | 消耗日志与统计页面 | 本次新增双语 Token 分项表、测量覆盖提示、未知 `—` 与真实零值区分；复用现有加载/错误重试/空状态、主题和响应式布局 |

## 明确差异与非目标

- 不复制 new-api 的管理员路由、数据库模式、余额/倍率/充值语义，也不把内部消耗成本变成用户钱包。
- new-api 的 `/v1/realtime`、`/v1/tools/search`、`/v1/tools/fetch`、Midjourney 插件路由及旧 `/v1/engines/...` 别名不在本次新增范围；不把已有 `/v1/alpha/search` 宣称为全部工具 API。
- new-api 中返回 NotImplemented 的 Files、Fine-tunes、Images variations 不构成已实现能力，鲮鲤不添加同名空壳路由。
- 不新增公开的充值式 Token 余额查询端点；使用经过授权的项目级分析和请求明细查看实际用量。
- Azure Entra 的旧 `DefaultAzureCredential` / `DeploymentIdentityCredential` 自动链已核对并明确拒绝，避免失败后改用 CLI 登录态或另一托管身份。应删除旧链 selector，或明确选择单一 client-secret/workload/managed identity；不声称完整旧认证链兼容，详见协议文档的迁移说明。
- Bedrock 跨协议仅非流式文本 Converse；工具、多模态与流式转换明确拒绝。SDK 内列出的供应商不等于经过本项目 contract-tested。
- 本地可选 tokenizer 只支持经检查的 GPT-4/GPT-3.5 文本 Chat 请求，其他形态沿用保守准入预留；不宣称通用硬上下文计数。

## 关键回归入口

- `src/providers/transport.rs`：原始二进制、状态/头、禁止重定向和取消。
- `src/providers/framing.rs`：分片 UTF-8、CRLF、多行、comment-only 超限、截断/无效帧。
- `src/providers/cloud.rs`、`src/providers/bedrock.rs`：显式认证、authority、安全拒绝与缺失用量。
- `src/api/gateway/tests/operations.rs`：配对总量、真实零、未知、缓存/推理不重复；既有 JSON/SSE、源协议、预算、项目隔离与亲和回归继续生效。
- `web/src/pages/TokenUsagePanel.test.tsx`：不拼凑半边用量、不重复加子集、零/未知、键盘可访问与中文。

实现状态、测试数量、浏览器证据和不可用的审查门禁必须按实际结果记录，不能由本表推断发布就绪。
