# 0034 Client 重连后的任务 MCP 注册

状态：维护中。

## 动机

TUI 的 `codex_tui` MCP 监听地址和 bearer token 随 Client 实例生成。
Client 退出或 daemon 重启后，恢复请求虽然携带当前注册信息，已加载的 thread
仍可能忽略配置覆盖，继续访问旧端口，或缺少该 MCP 注册。
上游报告：[openai/codex#58060](https://github.com/openai/codex/issues/58060)。

## 实现与边界

服务端记录提供任务 MCP 的连接。原连接断开后，新 Client 可以在 `thread/resume`
中重新注册；其它订阅该 thread 的观察连接不阻止交接。原连接仍在线时拒绝替换，
响应包含可识别的冲突原因，并提示断开原 Client 后重试。
跨会话发送收到此冲突时，TUI 省去注册配置重试一次，订阅目标后再提交回合。
目标继续使用原 Client 的 MCP 地址、鉴权和工具审批；原连接已断开时，
首次恢复仍可更新失效注册。

已有注册只更新 transport，保留启用状态、工具过滤和审批规则。
daemon 先恢复 thread、尚无注册时，使用 TUI 的固定任务审批规则补齐注册。
只处理带 bearer header 的内置 IPv4 loopback `/mcp` 服务；其它 MCP 与一般配置覆盖
仍沿用上游规则，不新增协议字段，不自动重放失败工具调用。

更新同时进入会话配置层并标记 MCP runtime 刷新，因此后续配置 reload
不会恢复旧地址。正在运行的回合无需为刷新而重建。

## 代码入口

- [恢复时的注册处理](../../codex-rs/app-server/src/request_processors/thread_processor/tui_mcp_rebind.rs)
- [跨会话发送](../../codex-rs/tui/src/dynamic_tools.rs)
- [连接归属](../../codex-rs/app-server/src/thread_state.rs)
- [会话配置发布](../../codex-rs/core/src/session/config_refresh.rs)
- [协议层回归](../../codex-rs/app-server/tests/suite/v2/mcp_tui_rebind.rs)

## 验证与维护

隔离的旧版已构建产物曾复现：恢复携带 B 的新 URL/token，却继续调用 A；关闭 A 后
错误指向旧端口，新 thread 使用 B 正常。该证据只证明原问题，不代替修复后验收。

Rust 回归覆盖新地址和 token、缺失注册、配置 reload、会话与工具审批保留，
以及原注册连接在线时拒绝替换、订阅后提交回合及保留注册、断开后有观察连接时的接管。
由 fork CI 编译和执行。

同步上游时核对 `thread/resume` 的已加载分支、连接清理与 MCP runtime 刷新。
缺失注册使用的固定审批规则须与 `tui/src/dynamic_tools_mcp.rs` 保持一致。
上游提供连接归属与重新注册能力后应移除重复实现，保留行为回归。
