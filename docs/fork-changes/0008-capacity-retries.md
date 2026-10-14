# 0008 容量错误无限退避重试

## 为什么改

服务端返回 `server_is_overloaded` 时，没有重试建议会立即终止，有建议也可能耗尽预算。
长任务因此需要手动续答。

## 如何改

普通会话、子代理和远程压缩 v2 采用独立的容量重试计数，间隔从 1 秒指数增长到 300 秒后封顶。
服务端建议的等待时间也受 5 分钟上限约束，不消耗其他错误的重试额度。
流式 HTTP 层直接把明确的 503 容量错误交给共用层，避免内部等待绕过上限。
等待时显示提示，可随时取消；内部自动审批继续遵守独立预算和超时。
自动审批会话使用上游 Guardian 分类函数识别，覆盖 `Internal(Guardian)` 和
`SubAgent(Other("guardian"))` 两种来源，避免容量重试绕过审批层的重试次数和超时。

## 入口和验证

- [共用重试](../../codex-rs/core/src/responses_retry.rs)、[单元回归](../../codex-rs/core/src/responses_retry_tests.rs)。
- [HTTP 层](../../codex-rs/codex-api/src/telemetry.rs)、[协议回归](../../codex-rs/core/tests/suite/retry_after.rs)。
  覆盖 HTTP、SSE、WebSocket、零预算、等待封顶、恢复和取消。
- 内部预算回归覆盖两种 Guardian 来源及记忆整理；保留 Guardian 审批会话重试回归。

## 上游同步

保留服务端错误分类，不能把额度不足、认证失败或策略拒绝也无限重试。
核对底层 HTTP 重试与采样循环的取消路径，并确保内部审批不被无限等待占住。
历史讨论：[PR #21](https://github.com/happy-proto/codex/pull/21)。
