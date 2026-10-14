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
- 实时值用窗口内相邻增量的时间差累计（不是窗口长度），窗口 5 秒；最新增量超过 1.5 秒不再显示，
  并安排一帧按时撤回状态栏上的旧数字；窗口内至少要有两个增量才报告速率。
- 增量按上游 4 bytes/token 的估算口径折算，再用「步骤报告的非推理 output ÷ 该步骤的正文增量估算」
  校准语言偏差，使中文等非 ASCII 内容不会系统性低估。推理 token 是 output 的子集，
  且其正文可能根本不下发，所以两侧都排除推理；工具调用之后的步骤同样跳过，
  因为工具参数也是不下发的 output token。
- 工具调用事件可能来自刚落地的步骤，也可能来自当前正在流式的步骤，因此当前步骤已产生正文时只排除当前步骤，
  否则两步都排除，不猜工具参数属于哪一步。
- 步骤用量用累计值之差计算：只有累计 output 增长才视为新步骤，速率限制刷新带来的重复通知
  不会重置当前步骤的估算；累计值回退说明换到了新会话，只重置基准不参与校准。
- 切换会话或恢复回放（包括恢复到进行中的回合）时，步骤只被观察到一部分，该步骤不参与校准，
  避免用整步用量除以局部估算。
- 推理条目完成会结束当前分段，避免把推理与正文之间的等待算进平均值；
  可重试的流错误会丢弃失败请求已流出的增量，并让该步骤既不参与校准也不进入平均值。
- 状态栏没有配置该速度项时，每条增量不做任何额外工作。
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

- `token_usage.total` 仍是累计用量、`last` 仍是最近一次采样的用量，且都区分 output 与 reasoning；
  步骤边界用累计 output 的增量，依赖累计值在会话内单调增长。
- reasoning token 仍是 output 的子集（`output_tokens_details.reasoning_tokens`），
  且推理正文仍可能不下发；校准两侧同时排除推理依赖这两个前提。
- 工具条目仍在响应内（正文增量之间）就可能上报 `ItemStarted`，所以工具排除只能按“当前步骤是否已产生正文”判断；
  上游若改在响应结束后才上报，双排除的保守分支会变少。
- 工具调用参数增量（`ResponseEvent::ToolCallInputDelta`）仍不下发给客户端。
  一旦上游下发，校准中的工具排除条件需要重写，否则会把工具参数算进校准。
- 上游若开始下发工具参数增量，或提供每步可见正文的精确 token 数，估算和校准可以直接换成精确值。
- 状态栏项枚举新增变体会在多个 match 处产生编译错误，按编译器提示补齐取值、预览、配色和说明。
