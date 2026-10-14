# 0010 回合平均生成速度

## 为什么改

生成速度是判断模型和链路状态最直接的指标，但上游只在采样步骤结束后给出 token 用量，
没有任何速度口径：回合结束的 runtime metrics 依赖 WebSocket 计时指标，且受
`runtime_metrics` 实验特性开关控制，默认关闭。

只显示回合平均值，不做实时刷新：实时数字在流式期间几乎每帧都在变，阅读成本高于信息量。

## 如何改

在 TUI 客户端测量，不改变 app-server 协议。回合结束时在完成行追加 `avg 41 tok/s`：

- 分子是该回合各步骤报告的非推理 output token：`token_usage.total` 的 output 增量减去
  reasoning 增量。推理 token 是 output 的子集，且推理正文可能根本不下发，无法测量它的生成时间，
  所以两侧都排除推理。
- 分母是该回合实测的正文流式时间：从步骤的首个正文增量到最后一条增量，按条目边界分段后求和，
  每个分段至少记 250ms。首字等待、工具执行和其它停顿都不在分母里。
- 只有同一尝试内既测量到时长又拿到用量的步骤才计入：只流出一条增量的步骤没有可测跨度；
  回放、恢复以及重试之后的步骤，其用量与时长来自不同尝试，直接跳过。
- 步骤边界用累计 output 的增量判断：累计值不增长说明是速率限制刷新重复上报的同一步骤，
  累计值回退说明换到了新会话，只重置基准。

## 入口和验证

- [测量与累计](../../codex-rs/tui/src/chatwidget/token_speed.rs)。
- [增量入口](../../codex-rs/tui/src/chatwidget/protocol.rs)、[条目和重试边界](../../codex-rs/tui/src/chatwidget/streaming.rs)、
  [回合边界](../../codex-rs/tui/src/chatwidget/turn_runtime.rs)、[恢复回合](../../codex-rs/tui/src/chatwidget/replay.rs)。
- [完成行](../../codex-rs/tui/src/history_cell/separators.rs)。
- 回归：[测量与累计单测](../../codex-rs/tui/src/chatwidget/token_speed.rs)、
  [完成行快照](../../codex-rs/tui/src/history_cell/separators_tests.rs)、
  [完成行与恢复回合](../../codex-rs/tui/src/chatwidget/tests/status_and_layout.rs)。

## 上游同步

口径依赖以下上游行为，同步时需要重新核对：

- `token_usage.total` 仍是累计用量且区分 output 与 reasoning，步骤边界依赖累计值在会话内单调增长。
- reasoning token 仍是 output 的子集（`output_tokens_details.reasoning_tokens`），
  且推理正文仍可能不下发。
- 正文增量仍逐条下发（当前实现逐条计时）；上游若合并增量，250ms 下限和「至少两条增量」的判断需要重新评估。
- 条目结束、流错误重试、恢复回放仍会触发对应的边界回调；这些回调缺失会让步骤的用量与时长错配。
