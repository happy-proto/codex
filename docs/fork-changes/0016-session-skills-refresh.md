# 会话 Skills 自动刷新保持输入响应

## 动机

首次 Skills 预取已经在后台运行，但会话配置完成后还会自动强制刷新一次。
该请求沿用主事件循环中的同步命令路径；等待 app-server 返回时，终端输入和重绘
都会暂停。因此可能先正常输入几个字符，随后出现短暂停顿。

## 行为

- 会话附加后的自动刷新通过 `RefreshSessionSkills` 事件启动后台请求，继续使用
  `forceReload=true` 获取当前工作目录的 Skills 元数据。
- 发起请求前检查工作目录，返回结果继续沿用 `SkillsListLoaded` 的过期目录保护、
  Skills 列表更新和无效清单警告处理。
- 用户主动触发的 Skills 刷新保留原有同步命令语义。
- 新会话、恢复和切换会话共享该自动刷新路径。

## 入口和验证

- [会话附加](../../codex-rs/tui/src/chatwidget/session_flow.rs)：发送自动刷新事件。
- [事件分发](../../codex-rs/tui/src/app/event_dispatch.rs)：检查目录后启动后台请求。
- [后台请求](../../codex-rs/tui/src/app/background_requests.rs)：与首次预取共用请求与结果事件。
- [回归测试](../../codex-rs/tui/src/app/tests/startup_skills_tests.rs)：以 WebSocket fixture
  保留 Skills 响应，验证会话附加产生的事件仍能处理完毕、输入能进入编辑器，且请求
  继续携带工作目录和强制刷新标记。测试先在未修复代码上因事件处理超时失败。
- [启动警告测试](../../codex-rs/tui/src/app/tests/startup_warnings_tests.rs) 和
  [会话生命周期测试](../../codex-rs/tui/src/app/tests/session_lifecycle_requests.rs)
  继续覆盖重复警告、旧目录结果和主动刷新路径。

真实 CLI 的补充验证可在隔离配置和临时项目中，通过 RPC 代理仅延迟第二次 Skills
响应，并用 PTY 比较输入与实际屏幕回显。该实验无需模型回合，可区分等待响应引起的
事件循环停顿与终端、输入法或整机调度问题。

## 上游同步

上游 [#18370](https://github.com/openai/codex/pull/18370) 将首次预取移到后台，
同步时仍需检查会话配置完成后的自动刷新是否沿用同步 `ListSkills` 路径。
若上游已覆盖该路径，核对目录保护、主动刷新语义和警告处理后移除本地实现，
保留响应延迟条件下的输入回归验证。
