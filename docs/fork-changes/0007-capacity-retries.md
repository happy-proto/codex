# 0007 容量错误无限退避重试

状态：已移除。恢复 `rust-v0.165.0-alpha.1` 的原生重试规则。

## 为什么改

服务端返回 `server_is_overloaded` 时，没有重试建议会立即终止，有建议也可能耗尽预算。
长任务因此需要手动续答。

## 如何改

此前给普通会话、子代理和远程压缩 v2 增加独立的容量错误无限退避，
并让 HTTP 层把明确的 503 容量错误直接交给共用层处理。

现已移除上述分支、计数和 HTTP 特判，恢复上游的错误分类、重试预算和服务端等待期限。
容量错误没有有效重试建议时可能直接结束当前回合；增大重试次数不能改变错误的可重试性。
等待期间的可见性由 [0021 重试等待提示](0021-advised-retry-visibility.md) 负责。

## 入口和验证

- [共用重试](../../codex-rs/core/src/responses_retry.rs)、[HTTP 层](../../codex-rs/codex-api/src/telemetry.rs)。
- [上游协议回归](../../codex-rs/core/tests/suite/retry_after.rs) 覆盖 HTTP、SSE、WebSocket 的重试预算、
  服务端建议和终止条件；展示测试单独维护在 0021。

## 上游同步

本编号保留为历史记录，不再维护容量错误重试特例。
历史讨论：[PR #21](https://github.com/happy-proto/codex/pull/21)。
