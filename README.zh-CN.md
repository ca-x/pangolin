<p align="center">
  <a href="README.md">English</a> · <strong>简体中文</strong>
</p>

<p align="center">
  <img src="web/public/logo.png" width="156" alt="Pangolin / 鲮鲤 logo" />
</p>

<h1 align="center">Pangolin · 鲮鲤</h1>

<p align="center">用一个精致、可观测、可自托管的 Rust 网关统一你的 AI API。</p>

Pangolin（中文正式名称“鲮鲤”）是一款单二进制 AI API 聚合网关。它提供 OpenAI 兼容入口、Anthropic Messages 入口、供应商与模型路由、虚拟 API Key、成本统计和请求追踪，并把中英文 React 管理界面直接嵌入 Rust 可执行文件。

## 功能

- OpenAI、Anthropic、Gemini、Jina 及兼容供应商的聊天、Responses、Embedding、图像、音频、视频、重排与异步任务入口
- 精确/正则/标签/条件模型路由，支持故障转移、轮询、权重、最少并发、延迟优先、粘性路由与熔断
- 项目、用户、角色、邀请、OIDC、API Key Profile、IP/额度/预算/日志策略
- 同一渠道配置多个加密凭据，支持优先级、轮换、批量启停、探测、配额和自动禁用
- 内置供应商/模型目录、品牌图标、能力与价格元数据；支持导入、导出、签名订阅、刷新与回滚
- Thread → Trace → Request → Execution → Usage/Cost 全链路观测与可解释路由预览
- SeaORM + SQLite 权威主库，DuckDB 独立派生分析库；项目与整实例加密备份/恢复
- 站点与 API Key 级请求日志策略，默认不保存敏感正文；可选精确会话回放与压缩
- Web 初始化向导或环境变量无人值守初始化
- 中英文、system/light/dark，以及 16 种界面风格 × 20 套配色
- 内嵌 React 控制台，单二进制与非 root Docker 镜像
- GitHub Actions 原生构建 Linux/macOS/Windows 二进制和 amd64/arm64 镜像

## 界面预览

### 桌面端

| 概览（浅色） | 概览（深色） | 渠道 |
| --- | --- | --- |
| ![鲮鲤桌面端概览](docs/screenshots/overview-desktop-light-en.png) | ![鲮鲤桌面端深色概览](docs/screenshots/overview-desktop-dark-zh.png) | ![鲮鲤桌面端渠道](docs/screenshots/channels-desktop-light-en.png) |

| 模型路由 | 请求记录 | 请求详情 |
| --- | --- | --- |
| ![鲮鲤桌面端模型路由](docs/screenshots/routing-desktop-dark-zh.png) | ![鲮鲤桌面端请求记录](docs/screenshots/requests-desktop-light-en.png) | ![鲮鲤请求详情](docs/screenshots/request-detail-desktop-light-en.png) |

| 追踪详情 | 访问控制 | 系统设置 | 关于 |
| --- | --- | --- | --- |
| ![鲮鲤追踪详情](docs/screenshots/trace-desktop-dark-en.png) | ![鲮鲤访问控制](docs/screenshots/access-desktop-light-zh.png) | ![鲮鲤系统设置](docs/screenshots/system-desktop-dark-zh.png) | ![鲮鲤构建信息](docs/screenshots/about-desktop-dark-zh.png) |

### 主题

界面风格（16 种）与配色（20 套）是两个独立维度，叠加明暗模式。风格取自 `ui-ux-pro-max` 风格库并按其视觉语言实现，配色取自其产品配色库；全部组合都通过自动化对比度审计（20 配色 × 2 模式 × 23 项配对）。详见 [design-system/pangolin/THEMES.md](design-system/pangolin/THEMES.md)。

| 鲮鲤默认 | 磨砂玻璃 | 黏土 |
| --- | --- | --- |
| ![鲮鲤默认风格](docs/screenshots/theme-house.png) | ![磨砂玻璃风格](docs/screenshots/theme-glass.png) | ![黏土风格](docs/screenshots/theme-clay.png) |

| 极光渐变 | 科幻 HUD | 杂志编辑 |
| --- | --- | --- |
| ![极光渐变风格](docs/screenshots/theme-aurora.png) | ![科幻 HUD 风格](docs/screenshots/theme-hud.png) | ![杂志编辑风格](docs/screenshots/theme-editorial.png) |

### 移动端

| 概览 | 渠道 | 多凭据 | 追踪详情 |
| --- | --- | --- | --- |
| ![鲮鲤移动端概览](docs/screenshots/overview-mobile-dark-zh.png) | ![鲮鲤移动端渠道](docs/screenshots/channels-mobile-light-en.png) | ![鲮鲤移动端多凭据管理](docs/screenshots/credentials-mobile-dark-zh.png) | ![鲮鲤移动端追踪详情](docs/screenshots/trace-mobile-dark-zh.png) |

## 快速开始

```bash
docker run --name pangolin \
  -p 8080:8080 \
  -v pangolin-data:/data \
  ghcr.io/ca-x/pangolin:latest
```

打开 `http://localhost:8080`，使用初始化向导创建首位管理员，然后依次添加供应商、模型映射和虚拟 API Key。

无人值守初始化：

```bash
docker run --name pangolin \
  -p 8080:8080 \
  -v pangolin-data:/data \
  -e PANGOLIN_ADMIN_EMAIL=admin@example.com \
  -e PANGOLIN_ADMIN_PASSWORD='replace-with-a-long-password' \
  ghcr.io/ca-x/pangolin:latest
```

使用 OpenAI SDK 时，把 Base URL 改为 Pangolin，模型名使用控制台配置的公开模型名：

```bash
curl http://localhost:8080/v1/chat/completions \
  -H 'Authorization: Bearer pg_your_virtual_key' \
  -H 'Content-Type: application/json' \
  -d '{"model":"fast-chat","messages":[{"role":"user","content":"Hello"}]}'
```

完整环境变量见 [.env.example](.env.example)。

## 从源码构建

需要 Rust 1.98、Node.js 22+、pnpm 11 和 C++ 工具链。DuckDB 使用 bundled 构建，首次编译耗时会明显长于普通纯 Rust 项目。

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo build --release --locked
./target/release/pangolin
```

开发时分别运行：

```bash
cargo run
pnpm --dir web dev
```

## 数据与安全

- `pangolin.db`：SeaORM 管理的 SQLite 权威主库，保存用户、会话、供应商、模型与虚拟密钥元数据。
- `observability.duckdb`：独立列式事件库，服务请求列表、分位延迟和时间聚合。
- `master.key`：未配置 `PANGOLIN_MASTER_KEY` 时自动生成，Unix 权限为 `0600`。备份数据库时必须同时安全备份该文件。
- 上游 API Key 使用 XChaCha20-Poly1305 加密；密码和虚拟 API Key 使用 Argon2id 哈希。
- 请求/响应正文采集默认关闭。设置 `PANGOLIN_CAPTURE_PAYLOADS=true` 前请评估隐私、合规和磁盘占用。
- v0.1 面向单节点部署。SQLite/DuckDB 文件不能由多个 Pangolin 进程共享写入。
- 带预算的虚拟 Key 会按 Key 串行执行余额复查与结算；为了避免无法可靠结算的流式消耗，预算 Key 不允许流式请求。单个非流式请求仍可能超过极小的剩余余额，预算用于成本护栏而非预付费硬账本。
- 请求事件默认保留 30 天，可用 `PANGOLIN_OBSERVATION_RETENTION_DAYS` 调整；DuckDB 不可用时网关继续提供代理服务，并在健康检查与指标中报告 degraded。

更完整的数据库权衡见 [docs/architecture/database.md](docs/architecture/database.md)。

## API 兼容边界

协议与供应商适配通过统一编排器执行，跨协议不支持的字段会明确报错，不会静默丢弃。Pangolin 不会在已经向客户端发送流字节后重试请求。没有真实凭据的供应商集成仅标记为契约测试通过，不宣称已经通过真实云服务验证。

## 参考与致谢

本项目在设计和实现时明确研究了以下开源项目：

- [looplj/axonhub](https://github.com/looplj/axonhub)（按其仓库 LICENSE 的适用范围，主要为 Apache-2.0；评估 commit `cb29b65d9adfb06f89bb1b467418e0816988f36c`）：参考企业访问控制、渠道/模型路由、可观测性、成本与备份的公开产品能力和测试不变量。
- [BerriAI/litellm 的 litellm-rust](https://github.com/BerriAI/litellm/tree/main/litellm-rust)（MIT；评估 commit `8c4c394ecc82c4d6acb5eb371d8781e487894a17`）：直接复用兼容的 types、core、llms、framing、auth、HTTP、token counter 与 cache crates，并通过 Pangolin 适配边界承接编排和缺失协议。
- [traceloop/hub](https://github.com/traceloop/hub)（Apache-2.0；评估 commit `e1be468f87de077ce066fbdebc858e913dc889a1`）：参考 Rust 网关的 provider registry、pipeline、Prometheus 和 OpenTelemetry 组织方式。
- [ca-x/raindrop](https://github.com/ca-x/raindrop)（MIT；评估 commit `73948bd650d2aa6b117b4ad63af6aff8b24e2938`）：参考嵌入式 React、原生多平台二进制、不可变备份目标快照、fencing、保留策略和 GitHub Actions 发布链路。
- [QuantumNous/new-api](https://github.com/QuantumNous/new-api)（AGPL-3.0；评估 commit `972aed1972820389ea0b603ca58f03f846fbf790`）：仅研究渠道/模型预设、分组倍率、令牌管理、亲和规则与运维交互；没有复制其 AGPL 源代码或资源。
- [farion1231/cc-switch](https://github.com/farion1231/cc-switch/tree/main/src-tauri/src/proxy)（MIT；评估 commit `06082e189d65e6d6dbadc35dacdac1ce6c79d89a`）：参考 Rust 代理流水线、故障切换、用量、媒体和会话处理设计。

Pangolin 是独立实现，与上述项目及其维护者不存在隶属或官方关联；第三方商标、版权和许可证归各自权利人所有。

## License

Apache-2.0
