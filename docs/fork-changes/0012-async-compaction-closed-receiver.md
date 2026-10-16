# 后台压缩失败后的接收器清理

状态：已由上游 `rust-v0.164.0-alpha.8` 吸收（[PR #55581](https://github.com/openai/codex/pull/55581)）。
本地重复实现和场景测试已移除，沿用上游实现与回归；生命周期诊断和失败退避分别由
[0016](0016-async-compaction-diagnostics.md) 与 [0017](0017-async-compaction-retry.md) 维护。

## 动机

后台压缩失败时，任务释放 oneshot 发送端。请求检查点通过 `try_recv` 观察到
`Closed` 后，接收器已经完成；若仍保留在会话中，随后前台压缩等待该接收器，
会触发 Tokio 的 `called after complete` panic，阻断原本可用的前台回退。

## 行为

- 检查点区分 `Empty` 与 `Closed`：前者保留未就绪的接收器，后者立即移除。
- 成功结果继续只消费一次，并沿用历史版本检查。
- 后台失败后，后续压缩通过原有前台流程完成；不调整压缩触发阈值、请求重试、
  历史安装、用量记录或后台任务的取消与排空策略。

## 入口和验证

- [异步压缩状态](../../codex-rs/core/src/session/async_compaction.rs)：
  `take_ready` 负责清理已经关闭的结果接收器，`wait_for_pending` 沿用原有等待路径。
- [会话场景测试](../../codex-rs/core/tests/suite/scenarios.rs)：
  `async_compaction_reuses_pending_work` 使用流响应闸门和后台任务完成屏障，
  在下一次检查点前完成失败的后台请求，再触发自动压缩。
  验证会话转向前台压缩、继续生成，且请求携带前台压缩结果、用量按既有规则记录。
  同组场景覆盖尚未就绪、成功、等待期间失败、过期结果、手动压缩与中断。
- [自有 CI](../../.github/workflows/fork.yml) 的常规验证运行异步压缩场景；
  Rust 编译和回归测试交给 GitHub Actions，本地只做格式与静态检查。

## 上游同步

沿用上游先取出接收器、仅在 `Empty` 时放回的实现；`Closed` 与成功结果均不再留在状态中。
上游 `ReadyFailure` 场景覆盖检查点前失败后的前台回退，继续纳入自有 CI。
本修复证明接收器生命周期问题，不据此推断缺少调用栈的历史会话卡死原因。
