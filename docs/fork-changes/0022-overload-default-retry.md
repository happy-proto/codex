# 0022 容量错误缺省等待重试

状态：维护中。

## 为什么改

上游将没有有效 `Retry-After` 的 `server_is_overloaded` 视为不可重试错误，
即使尚有重试预算也会立即结束回合。WebSocket 返回 503 容量错误时可触发此行为。

## 如何改

Responses 共用重试入口只为缺少有效等待建议的 `ServerOverloaded` 补上一分钟期限，
复用现有的等待提示、重试预算和传输回退流程；普通推理与远程压缩 v2 使用同一规则。
等待前显示原因、次数和倒计时，用户仍可中断回合。

有效的服务端建议始终优先，包括零等待和超过一分钟的等待，已过去的时间不会重新计时。
无效建议按缺少建议处理；不改变其他错误的分类，不增加无限重试。
共用流重试预算默认从 5 次提高到 50 次；显式配置继续优先，配置上限仍为 100 次。
HTTP 底层请求预算仍为 4 次，避免两层预算同时扩大。
普通流错误的本地指数退避最多 60 秒；只限制本地退避，不截短服务端有效的 `Retry-After`。
WebSocket 耗尽预算后的 HTTPS 回退同样遵守这一分钟期限，回退后的预算仍沿用现有规则。
HTTP 底层请求重试保持上游策略，默认等待在错误进入共用层后生效。

## 入口和验证

- [共用重试入口](../../codex-rs/core/src/responses_retry.rs)。
- [默认预算](../../codex-rs/model-provider-info/src/lib.rs) 与 [预算回归](../../codex-rs/model-provider-info/src/model_provider_info_tests.rs)：默认 50 次、显式配置及上限。
- [定向回归](../../codex-rs/core/src/responses_retry_tests.rs)：默认等待不早于 60 秒、本地退避上限、首次提示、传输回退、预算耗尽，以及保留零、短、长服务端期限。
- [协议回归](../../codex-rs/core/tests/suite/retry_after.rs)：HTTP 预算、SSE 及 WebSocket 的 `error`、`response.failed`、无效建议和从服务端建议切换到默认等待后的恢复。
- [等待提示](0021-advised-retry-visibility.md)：每次等待都可见。

## 上游同步

保留改动在共用重试入口的边界，不向协议解析层伪造服务端响应或改变 HTTP 底层重试。
上游若原生支持缺省容量等待，核对等待时间、次数限制与首次提示后再移除重复实现。
不恢复 [0007](0007-capacity-retries.md) 中已移除的无限退避。
