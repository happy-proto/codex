# 0013 WebSocket 连接主动轮转

状态：维护中。

## 为什么改

Responses WebSocket 连接会跨请求和 turn 复用。
[上游协议](https://developers.openai.com/api/docs/guides/websocket-mode#errors-to-handle)
将连接寿命限制为 60 分钟，达到限制后返回 `websocket_connection_limit_reached`。
原有错误重试可以重新连接，但会多出一次失败请求及重试等待。

## 如何改

- 在握手成功后记录 Tokio 单调时钟；连接与建立时间一起跨 turn 缓存。
- 连接年龄达到 30 分钟时，在下一次预连接、预热或正常请求前轮转。
  年龄从握手成功起计算，不随请求、预热或 turn 开始而刷新；新连接重新计时。
- 轮转复用既有 WebSocket 状态重置，清除旧连接的请求基线、响应 ID 和预热标志。
  新连接的第一条请求携带当前完整上下文，不引用旧 `previous_response_id`；
  后续请求仍可使用增量续接。上下文包含已记录的消息、工具调用与工具结果，
  不通过重放工具执行恢复历史；已有上下文压缩结果继续作为当前历史使用。
- 同一 turn 的路由状态保留，认证所有权变化仍按原逻辑清理。
  轮转原因使用固定的 `connection_age`，沿用已有续接诊断指标。
- 不新增定时器、后台轮转任务或配置项，不因连接年龄中断正在进行的响应，
  也不为轮转额外等待在途响应或关闭握手。正常调用方在响应完成后发起下一次请求。
  请求开始时连接未满 30 分钟，但响应长时间在途，仍可能达到上游 60 分钟上限；
  主动轮转不承诺消除所有到期错误。
- 原有网络故障、到期错误重试和 HTTP fallback 保持原路径。

## 入口和验证

- [连接缓存和请求边界](../../codex-rs/core/src/client.rs)：`WebsocketSession`、
  `websocket_connection`、`is_websocket_prewarmed`。
- [WebSocket 回归测试](../../codex-rs/core/tests/suite/client_websockets.rs)：
  `responses_websocket_rotation_*` 使用可控时间验证阈值前复用、达到阈值轮转、
  跨 turn 与长时间空闲、完整消息与工具上下文恢复、轮转后增量续接、
  预连接和预热，以及跨阈值在途响应完整结束。
- 原有 `responses_websocket_connection_limit_error_reconnects_and_completes`
  验证上游到期错误发生后的重试恢复。Rust 测试交给 GitHub Actions，本地不编译。

## 上游同步

关注上游是否增加连接年龄管理。复用连接与增量请求准备的顺序变化时，
确保年龄检查和状态清理发生在生成 `previous_response_id` 与增量输入之前。
预热的提前返回必须使用相同的年龄判断，不能把接近到期的连接当作已准备就绪。
保持在途响应和连接年龄策略分离，不引入到时强制断开或无界关闭握手等待。
