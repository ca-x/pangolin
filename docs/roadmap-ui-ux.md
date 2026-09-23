# Pangolin UI/UX Roadmap

> 建立日期：2026-09-23（v0.5.0 发布后）
> 来源：本次以 `ui-ux-pro-max` + `emil-design-eng` 两个规范对控制台做的评估、对
> [axonhub](https://github.com/looplj/axonhub)（Apache/LGPL，行为参照）与
> [new-api](https://github.com/QuantumNous/new-api)（AGPL，仅研究产品行为，不抄代码）
> 的前端清单审计，以及 `docs/refactoring-ui-audit.md` 的既有结论。
> v0.5.0 已交付：九个结构化文档编辑器、目录导入分组重构、多渠道原子建模型、
> 渠道权重、按压反馈/语义色/按路由标题。本文件只列**未做**的部分。

条目按优先级分组；每条含现状、目标、实现要点与验收标准。参照文件路径为
外部仓库的相对路径，仅作行为参照。

## P0 — 高感知差距，直接对齐参照产品

### 1. 渠道表单分页（渐进式分区）

- **现状**：渠道创建/编辑对话框是单列长表单（名称/类型/base_url/优先级/设置编辑器），
  渠道策略（`channel-settings`）在另一个独立资源页里。
- **目标**：对齐 new-api 的 4-tab 抽屉形态（`web/src/features/channels/drawers/channel-configuration.tsx`）：
  连接与模型 / 路由与映射 / 请求与响应 / 其他设置，每个 tab 带就绪状态点。
- **要点**：在 ResourcePage 之上为渠道建专用编辑视图；把现有编辑器（settings、
  endpoint mappings、model rules、parameter overrides、retry、auto-disable、代理）
  按上述四组重新归位，而不是新增表单逻辑。
- **验收**：375px 单列可用；每个 tab 有 Ready/Incomplete 状态点；提交契约不变
  （同一批字段、同一批后端校验）。

### 2. 渠道健康火花条

- **现状**：渠道表有健康徽标（`HealthCell`），无历史。
- **目标**：axonhub 的 15 根探测火花条（`frontend/src/features/channels/components/channel-health-cell.tsx`）：
  按成功率着色的柱状微图 + tooltip（探测时间/成功率/平均 TTFT/tok·s⁻¹）。
- **要点**：数据源是已有的 `channel_probes`（`operations_api` 已有投影）；
  手写 SVG，遵守 MASTER.md 图表规则（不动画、有数据表回退）。
- **验收**：明暗两主题 3:1 非文本对比度；探测为空显示 `—` 而不是空图。

### 3. 请求详情会话视图

- **现状**：请求/响应体是 JSON 树（`PayloadViewer.tsx`）。
- **目标**：把 messages 数组渲染为对话流（角色徽标/分色、工具调用块、reasoning 折叠），
  参照 axonhub `frontend/src/features/requests/components/request-conversation-viewer.tsx`。
- **要点**：仅作为 JSON 视图旁的第二个 tab，不替换原始视图；>1000 字符的块默认折叠。
- **验收**：openai/anthropic/responses 三种协议的 messages 形状都能渲染；
  未知形状回退 JSON 视图而不是报错。

### 4. URL 可寻址的 Tabs（深链）

- **现状**：v0.5.0 已做按路由的浏览器标题；但 6 个页面的 tab 仍是组件内 state
  （`ChannelsPage`/`ModelsPage`/`SystemPage`/`AccessPage`/`PromptsPage`/`OperationsPage`），
  刷新即丢。
- **目标**：tab 绑定 `?tab=`，仓库内已有模式（`OperationsPage` 的筛选、`AnalyticsPage`）。
- **验收**：刷新/分享链接回到同一 tab；浏览器后退跨 tab 可用。

## P1 — 操作效率与一致性

### 5. 表格列排序

- **现状**：`shared.tsx` 的表只接了 core/filtered 行模型，无任何排序表头（无 `aria-sort`）。
- **目标**：请求日志、模型表的 2–3 个关键列可排序，服务端排序参数（`q=` 已有先例）。
- **验收**：排序状态进 URL；空值排序稳定（`—` 恒排最后）。

### 6. 移动端卡片回退补齐

- **现状**：`ResourcePage` 有 MobileResources 卡片列表，但 KeysPanel
  （`AccessPage.tsx`，1020px 宽表）、邀请、webhook 投递三个自绘表没有，375px 是横向拖动。
- **目标**：复用 MobileResources 或交给 ResourcePage 外壳。
- **验收**：375px 无横向页面滚动（MASTER.md L123 的硬约束）。

### 7. 原始 ID 输入替换为选择器

- **现状**：成员/绑定表单用自由文本收 `user_id`/`role_id`（数据其实已加载）；
  KeyLoggingForm 贴 `api_key_id`；`expires_at` 是 epoch 秒输入。
- **目标**：Select 喂已加载查询；过期时间用 `datetime-local`（`OperationsPage` 已有先例）。
- **验收**：不再出现“手填 UUID”的表单；epoch/本地时间双向换算有测试。

### 8. 统一实时刷新控件

- **现状**：三种实时刷新习语并存；3 秒轮询面板既无暂停也无更新时间；
  共享的 `AutoRefreshControl`（持久化、最小 1s 转动）只有两处在用。
- **目标**：live-requests 与请求列表都改用共享控件；时间戳走 i18n 的 `formatUpdatedAt`。
- **验收**：控制台内不再有第三种刷新按钮形态。

### 9. 空状态补齐

- **现状**：`PresetsPanel`（`ChannelsPage.tsx`）与目录订阅列表无空状态。
- **目标**：共享 `EmptyState` + 主行动按钮。
- **验收**：空数据渲染不出裸网格。

## P2 — 值得“偷学”的参照细节（纯 UX）

| # | 细节 | 参照 | 说明 |
| --- | --- | --- | --- |
| 10 | Key 创建后**脱敏可复制**的快速接入代码 tab（Claude Code/Codex/各 SDK） | axonhub `apikeys-view-dialog.tsx` | 显示脱敏、剪贴板拿真值 |
| 11 | 抽屉**跨页 ←/→ 导航**并按需拉取相邻页 | axonhub `request-body-drawer.tsx` | 请求详情浏览不丢列表位置 |
| 12 | **重试链执行卡片**：编号尝试 + 每次延迟/TTFT 磁贴 + 凭据后缀 | axonhub `request-detail-content.tsx` | 数据已有（request_executions） |
| 13 | 关联编辑器旁的**实时路由预览**（防抖、可过滤） | axonhub `models-association-dialog.tsx` | Pangolin 已有 channel-preview 端点可复用 |
| 14 | **批量粘贴导入** + 逐行错误 + 重复名检测 | axonhub `channels-bulk-import-dialog.tsx` | 渠道导入场景 |
| 15 | 仪表盘**分区折叠持久化**到 localStorage | axonhub `dashboard/index.tsx` | 动画 ≤300ms、chevron 旋转即可 |
| 16 | 登录失败**内联错误** + 按钮 `loading`（替代角落 toast 与禁用文案） | Li 审计 gap #10 | `Auth.tsx` |
| 17 | 行内双击编辑权重（Enter/Escape + 保存 spinner） | axonhub `channels-columns.tsx` | 渠道优先级列的快改路径 |

## 后端决策项（需先定数据契约，再动 UI）

### 18. 请求头覆盖（header_override）

- new-api 渠道有独立的 header 覆盖文档（支持 `{api_key}`/客户端头变量）；
  Pangolin 渠道文档（`channel_settings`）目前只有 `pass_user_agent`。
- 需先决策：进 `channel_settings` 新键还是独立列；鉴权头覆盖与“不透传凭据”的
  安全边界如何表述。**不决策不动手。**

### 19. 渠道级重试策略

- new-api 的重试是全局设置（页头徽标提示）；Pangolin 已是 per-channel
  `retry_statuses`（更细）。保持现状即可，列在这里只为避免“对齐”时误抄全局语义。

## 工程健康

### 20. web 全量套件并行抖动

- 事实：本机全量 `vitest run` 在并行负载下失败 4–14 个测试；**基线 HEAD
  （不含 v0.5.0 改动）失败 14 个**，比改动后更多；所有失败用例单独跑稳定通过。
  典型：roleBindings（4s 权限延迟逻辑）、storageTargetForm、channelHealth。
- 方向：给重时序测试加宽容超时/串行标记（`describe.sequential` 或 vitest
  `poolMatchGlobs`），或把 deferred-promise 类测试改为显式 `vi.advanceTimersByTime`。
- 验收：CI 上 `pnpm --dir web test` 连续 3 次全绿。

### 21. 死 CSS 层清理

- `.sidebar/.brand/.stat/.empty-state/.status/.select-content/.tooltip-content`
  等一批规则已无 TSX 引用（Radix 迁移遗留），且 `.button:active` 曾只服务死类
  （v0.5.0 已补 Mantine 根）。删除死层可让“契约 vs 实现”的差距可审计。
- 验收：删除后 `grep` 佐证零引用；`pnpm build` 体积下降。

## 遵守的约束

- 所有新 UI 双语（zh-CN/en）、双主题达标、375/768/1440 可用；
  动效走 MASTER.md 的 token（≤300ms、transform/opacity、reduced-motion 降级）。
- 参照仓库只学行为：axonhub 是 Apache/LGPL 测试参照，new-api 是 AGPL，
  **一律不复制代码与图片资源**。
- 动数据库前先过 `ddia-principles` 审核（v0.5.0 的 `providers.priority` 即此流程）。
