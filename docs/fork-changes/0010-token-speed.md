# 0010 生成速度显示

## 为什么改

生成速度是判断模型和链路状态最直接的指标，但上游只在采样步骤结束后给出 token 用量，
没有任何速度口径：回合结束的 runtime metrics 依赖 WebSocket 计时指标，且受
`runtime_metrics` 实验特性开关控制，默认关闭。

## 如何改

在 TUI 客户端按流式增量测量速度，不改变 app-server 协议：

- 新增状态栏项 `tokens-per-second`（接受别名 `tps`）。流式期间显示 `41 tok/s`，
  没有增量时该项为空，不会留下过期数字。
- 回合结束的完成行追加 `avg 41 tok/s`，与状态栏的实时值来自同一套测量。
- 实时值用窗口内相邻增量的时间差累计（不是窗口长度），窗口 5 秒；最新增量超过 1.5 秒不显示；
  单增量段用 250ms 到 1s 之间的兜底时长，避免除零和失真的峰值。
- 增量按上游 4 bytes/token 的估算口径折算，再用「步骤报告的 output + reasoning ÷ 该步骤的流式估算」
  校准语言偏差，使中文等非 ASCII 内容不会系统性低估。
- 只有确认流式内容覆盖了该步骤全部报告 token 的步骤参与校准：工具调用之后的步骤，
  以及报告了推理 token 但客户端没有收到推理增量的步骤（隐藏推理）都跳过。
- 回合平均值只累计有实测跨度的生成段，工具执行和其它等待时间不计入分母。

## 入口和验证

- [采样与校准](../../codex-rs/tui/src/chatwidget/token_speed.rs)。
- [增量入口](../../codex-rs/tui/src/chatwidget/protocol.rs)、[回合边界](../../codex-rs/tui/src/chatwidget/turn_runtime.rs)。
- [状态栏项定义](../../codex-rs/tui/src/bottom_pane/status_line_setup.rs)、[取值](../../codex-rs/tui/src/chatwidget/status_surfaces.rs)、
  [完成行](../../codex-rs/tui/src/history_cell/separators.rs)。
- [采样与校准回归](../../codex-rs/tui/src/chatwidget/token_speed.rs)、
  [状态栏和完成行回归](../../codex-rs/tui/src/chatwidget/tests/status_and_layout.rs)。

## 上游同步

口径依赖以下上游行为，同步时需要重新核对：

- `ThreadTokenUsageUpdated.last` 仍是最近一次采样的用量，并且仍区分 output 与 reasoning。
- 工具调用参数增量（`ResponseEvent::ToolCallInputDelta`）仍不下发给客户端。
  一旦上游下发，校准中的工具排除条件需要重写，否则会把工具参数算进校准。
- 推理增量仍受当前推理条目和 `show_raw_agent_reasoning` 控制，隐藏推理时不能参与校准。
- 状态栏项枚举新增变体会在多个 match 处产生编译错误，按编译器提示补齐取值、预览、配色和说明。
