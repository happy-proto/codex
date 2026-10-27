# 0020 WebSocket 消息过大错误

状态：维护中。

## 为什么改

服务端以 WebSocket 关闭码 1009 拒绝消息时，原实现丢弃关闭码和原因，
将它视为普通流断开，重复发送相同请求并可能回退 HTTPS。
上传期间及时读取关闭帧也能避免在发送失败后丢失具体原因。

## 如何改

Responses WebSocket 错误保留结构化关闭码和经过转义、限长的原因。
1009 映射为现有不可重试的请求错误，明确提示减少图片大小或上下文、开启新会话；
共用重试循环直接返回，不自动重发，也不通过 HTTPS 回退再次发送。
其它关闭码与没有关闭帧的断连保持原有重试策略，并尽可能展示已知关闭信息。
遥测只记录关闭码，不记录服务端关闭原因。

从上游 `0.173.0-alpha.4` 起复用其 WebSocket pump，在上传背压和写失败期间继续读取事件并刷新 Pong。
当前请求开始发送后收到的关闭帧进入结构化分类。发送首帧前检测到连接已关闭时，
按上游策略将连接视为失效并允许重连。
已经收到 `response.completed` 的正常响应不会因后续关闭而变为失败。

没有观测到关闭码的网络写失败仍按临时错误处理，不能仅凭请求大小推断为 1009。
上游 `0.170.0-alpha.2` 起，发送前按服务端声明的消息上限检查请求，
超限时沿用上游 HTTP 回退；本改造仍保留实际收到关闭码 1009 后停止重发的行为。
不自行声明固定服务端字节上限，不自动删除历史图片、不改 Token 估算或上下文协议，
也不假定 WebSocket 分帧或 HTTP 可以绕过消息大小限制。

## 入口和验证

- [WebSocket pump 与响应处理](../../codex-rs/codex-api/src/endpoint/responses_websocket.rs)。
- [API 错误与核心错误映射](../../codex-rs/codex-api/src/api_bridge.rs)。
- [传输回归](../../codex-rs/codex-api/src/endpoint/responses_websocket_close_tests.rs)：
  通过真实 WebSocket 帧覆盖关闭码、原因转义、临时关闭及正常完成；
  [上传测试](../../codex-rs/codex-api/src/endpoint/responses_websocket_upload_tests.rs)覆盖阻塞上传和发送首帧前关闭。
- [共用重试回归](../../codex-rs/core/src/responses_retry_tests.rs)：
  采样与远程压缩在 1009 时直接返回，不消耗重试预算、不触发传输回退。
- Rust 编译和回归在 GitHub Actions 执行；本机仅做格式与轻量检查。

## 上游同步

上游新增关闭错误分类时优先复用，保留关闭码的结构化判断及 1009 的终止行为。
`doctor` 网络诊断保留关闭码，隐藏服务端原始关闭原因，遵循上游诊断脱敏规则。
不能将所有 WebSocket 关闭一律设为不可重试，也不能把本地接收容量错误当作服务端拒绝请求。
若 pump 读写模型变化，继续验证上传中收到关闭帧、Ping/Pong 与正常完成后的连接行为。
