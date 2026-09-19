<p align="center">
  <img src="web/public/logo.png" width="156" alt="Pangolin / 鲮鲤 logo" />
</p>

<h1 align="center">Pangolin · 鲮鲤</h1>

<p align="center">用一个精致、可观测、可自托管的 Rust 网关统一你的 AI API。</p>

Pangolin（中文正式名称“鲮鲤”）是一款单二进制 AI API 聚合网关。它提供 OpenAI 兼容入口、Anthropic Messages 入口、供应商与模型路由、虚拟 API Key、成本统计和请求追踪，并把中英文 React 管理界面直接嵌入 Rust 可执行文件。

## 功能

- OpenAI 兼容的 `/v1/chat/completions`、`/v1/responses`、`/v1/models`
- Anthropic 兼容的 `/v1/messages`，以及基础非流式 OpenAI → Anthropic 转换
- 多供应商同名模型的优先级故障转移
- 加密上游密钥、Argon2id 管理员密码和虚拟 API Key
- SeaORM + SQLite 控制主库，DuckDB 独立请求观测库
- 请求量、错误率、P95 延迟、Token 和微美元成本
- Web 初始化向导或环境变量无人值守初始化
- 中英文、system/light/dark、bronze/slate/jade 主题
- 内嵌 React 控制台，单二进制与非 root Docker 镜像
- GitHub Actions 原生构建 Linux/macOS/Windows 二进制和 amd64/arm64 镜像

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

v0.1 优先保证 OpenAI 兼容上游的透明传输。OpenAI → Anthropic 的非流式文本与基础工具调用由独立适配层转换；跨协议流式转换暂不提供，使用 Anthropic SDK 时应调用 `/v1/messages`。不会在已经向客户端发送流字节后重试请求。

## 参考与致谢

本项目在设计和实现时明确研究了以下开源项目：

- [BerriAI/litellm 的 litellm-rust](https://github.com/BerriAI/litellm/tree/main/litellm-rust)（MIT）：参考多供应商协议转换与类型边界。评估 commit `8c4c394ecc82c4d6acb5eb371d8781e487894a17`。由于其 crate 尚未独立发布、Git 依赖会解析整个大型工作区，v0.1 未把它作为默认构建依赖；Pangolin 的适配层保留了未来替换边界。
- [traceloop/hub](https://github.com/traceloop/hub)（Apache-2.0）：参考 Rust 网关的 provider registry、pipeline、Prometheus 和 OpenTelemetry 组织方式。
- [looplj/axonhub](https://github.com/looplj/axonhub)：参考渠道/模型/API Key、健康路由、请求追踪和成本控制的产品能力。本项目为独立实现，不复制其源代码。
- [ca-x/raindrop](https://github.com/ca-x/raindrop)：参考嵌入式 React 资源、原生多平台二进制、非 root Docker 与固定 SHA 的 GitHub Actions 发布链路。

这些项目的商标、版权和许可证归各自权利人所有。

## License

Apache-2.0
