# 0008 已打开会话的名称同步

状态：维护中。

## 为什么改

改名通知只在对应 app-server 实例内广播。其他实例改名后，agents 列表可能更新，
但当前 ChatWidget 缓存仍旧，status line 和 terminal tab title 随之滞后。

## 如何改

标题中展示 thread 名称时，每 30 秒后台读取当前显示会话的 `thread/read(includeTurns=false)`。
主事件循环在空闲时仍驱动检查，不依赖输入；读取不重叠，5 秒超时，失败静默等待下一轮。
保留通知的即时更新。名称变化时同时刷新两处标题，名称未变时不重画，也不推进排队输入。
用 thread ID 和名称版本丢弃较新改名、会话切换或 ChatWidget 替换后的过期查询结果。

## 入口和验证

- [轮询状态和读取](../../codex-rs/tui/src/app/thread_name_refresh.rs)、[主循环](../../codex-rs/tui/src/app/startup.rs)。
- [名称应用](../../codex-rs/tui/src/chatwidget/session_flow.rs)、[标题回归](../../codex-rs/tui/src/chatwidget/tests/terminal_title.rs)。
  覆盖两处标题、清除名称、版本保护、展示条件、间隔与不重叠读取。

## 上游同步

不要恢复整段历史或重新配置会话来刷新标题，避免影响正在执行的任务。
关注 app-server 的元数据缓存、通知路由、ChatWidget 替换和终端标题刷新路径。
历史讨论：[PR #23](https://github.com/happy-proto/codex/pull/23)。
