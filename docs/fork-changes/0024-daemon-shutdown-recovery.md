# 0024 daemon 关停拒绝后的 TUI 重连

状态：维护中。

## 动机

daemon 重启时可能先拒绝 `turn/steer` 等请求，再关闭连接。拒绝响应为
JSON-RPC `-32600`，且 `data.reason` 为 `serverShuttingDown`。
TUI 原先只恢复传输错误，这类拒绝会沿请求错误路径传播并退出客户端。

## 实现

公共连接恢复入口将关停拒绝与传输错误一起交给已有重连流程。
复用上游错误识别函数：优先检查结构化 reason；仅在没有 data 时兼容旧版精确消息。
其它 JSON-RPC 错误不触发该恢复，内嵌 app-server 仍不执行外部连接重连。

保留草稿和待确认消息，沿用既有历史回读与输入恢复规则，不立即重发被拒绝的请求。
重连时若 `thread/resume` 仍遇到关停拒绝，继续尝试连接，避免把关停响应误判为会话不可用。

## 代码入口

- [公共恢复与重连](../../codex-rs/tui/src/app/reconnect.rs)
- [关停错误识别](../../codex-rs/tui/src/app_server_session.rs)
- [回归测试](../../codex-rs/tui/src/app/tests/disconnect_tests.rs)
- [fork CI](../../.github/workflows/fork.yml)

## 验证与维护

`draining_turn_submission_reconnects_without_exiting_or_replaying` 使用真实 WebSocket
连接模拟拒绝，覆盖 `turn/start`、`turn/steer` 和结构化、旧消息两种响应。
服务端保持连接打开，确认拒绝本身即可进入重连；同时检查草稿、待确认 steer 保留且不立即重发。
`shutdown_recovery_does_not_mask_other_server_errors_or_reconnect_embedded` 覆盖错误分类与内嵌边界。
fork CI 运行完整 `app::tests::disconnect::` 测试组。

同步上游时检查公共恢复入口和 `thread/resume` 的错误分支；若上游已覆盖关停拒绝，
移除重复逻辑并保留行为回归测试。不要仅按 `-32600` 判断关停，以免掩盖其它请求错误。
