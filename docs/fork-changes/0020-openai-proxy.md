# OpenAI 模型请求专用代理

状态：维护中。

## 动机与配置

慢速出口上传较长上下文时，模型请求可能反复超时。进程级 `HTTP_PROXY`、`HTTPS_PROXY` 会被 shell 和其它子进程继承，不适合只优化模型请求。

在 `config.toml` 顶层配置：

```toml
openai_base_url = "https://relay-scope.dcjanus.com/backend-api/codex"
openai_proxy_url = "http://127.0.0.1:7897"
```

`openai_proxy_url` 是代理服务器地址，仅用于内置 `openai` provider 的模型传输，包括 HTTP Responses、压缩、WebSocket 与预热。CLI 与 daemon 共用同一实现，不依赖启动 shell 的代理环境变量。自定义 provider、登录与凭据刷新、模型目录、MCP、插件及 shell 命令保留各自原有网络配置；本配置不会写入进程或子进程环境。

配置存在即生效，无需额外 feature 开关。支持绝对的 `http`、`https`、`socks5`、`socks5h` 代理 URL；空值、缺少主机及不支持的 scheme 在加载配置时明确报错，错误不包含代理凭据。未配置时沿用现有网络行为。显式配置优先于环境变量、`NO_PROXY` 和系统代理；代理失败不降级为直连。已存在的应用网络策略仍生效。

代理服务必须独立运行。配置应由更新后的 CLI 或 daemon 加载；重启 daemon 会中断当前工作，不在配置写入时自动重启。若此前在 `~/.codex/.env`、终端或系统环境中设置了通用代理，须单独移除，才能避免其它请求继续继承它们。

## 实现与验证

- `config/src/config_toml.rs` 和 `core/src/config/mod.rs`：配置与校验；`model_http_client_factory` 仅为内置 OpenAI provider 派生模型网络工厂。
- `core/src/session/session.rs`：把模型工厂交给 `ModelClient`，覆盖 HTTP 与 WebSocket 共用路径。
- `http-client/src/outbound_proxy.rs`：工厂实例级的显式路由，保留网络策略和 Cookie，代理地址从调试输出中脱敏。
- `login/src/auth/default_client.rs` 与 `http-client/src/route_aware_client_pool.rs`：显式路由不进入旧的默认代理或自定义 CA 回退分支。
- `cli/src/doctor.rs`：Responses WebSocket 检查使用同一模型网络工厂；普通 HTTP 可达性检查仍遵循普通网络配置。

定向测试覆盖代理优先级、工厂隔离、无效配置、凭据脱敏、HTTP 请求实际抵达代理，以及公开 WebSocket 连接器经 HTTP/HTTPS CONNECT 建立 TLS 握手：

```sh
cargo test -p codex-core --lib openai_proxy
cargo test -p codex-http-client configured_proxy
cargo test -p codex-login explicit_proxy_routes_default_api_client
cargo test -p codex-websocket-client proxy_tunnels_secure_websocket_before_handshake
```

按仓库约定，Rust 编译和测试交给 CI；本地只做格式、元数据、定向脚本和静态检查。上述测试的存在不表示当前改动已通过 CI、发布或安装。

## 同步上游时

此为 fork 配置，上游暂无对应配置字段。保留普通工厂与模型工厂的作用域边界；尤其不能把代理附加到 `Config::http_client_factory`，或改用进程环境变量。上游改变模型客户端构造、默认客户端回退或 WebSocket 路由时，重新核对两种传输及自定义 provider 的隔离测试。同步 schema 时保持新增字段和中文文档注释一致。
