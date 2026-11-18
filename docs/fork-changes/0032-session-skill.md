# 0032 内置会话管理 skill

TUI 的任务 MCP 由各客户端托管。客户端全部退出时，常驻 app-server
仍有会话状态，但模型无法调用这些跨会话工具。新实现将会话操作作为普通
app-server RPC 执行，TUI 可以独立退出或重新连接。

**Breaking change** 交互式 CLI 默认不启动 `codex_tui` 任务 MCP。
模型使用内置 `codex-session` skill 和 `codex task`。依赖旧工具的调用者
可在启动 fork TUI 时使用 `-c features.tui_task_mcp=true`。
该兼容开关启用原有客户端托管实现。

`codex task` 默认连接当前 `CODEX_HOME` 的常驻 app-server，
也支持显式选择 `--remote`。每条命令复用一条连接，不启动另一个 app-server。
跨会话发送恢复目标并提交带来源的回合，返回服务端受理的回合 ID。
创建会话继承源会话的权限配置和审批策略。目标审批由 app-server 处理。
连接丢失后先检查目标回合再决定是否重试。会话管理替代覆盖 TUI 的任务工具。
上游的 managed worktree 工具仍需启用 TUI MCP，目前没有对应的 worktree 创建 RPC。
原有 `wait_threads` 的游标由 skill 使用回合 ID 和历史分页核对。

- [内置 skill](../../codex-rs/skills/src/assets/fork-native/codex-session/SKILL.md)
- [CLI 操作实现](../../codex-rs/tui/src/session_task_commands.rs)
- [旧 transport 开关](../../codex-rs/tui/src/app/startup.rs)
- [上游同步检查](../../.agents/skills/codex-upstream-sync/SKILL.md)

验收在隔离 daemon 中创建两个已持久化的会话，退出所有 TUI 后用
`codex task send` 发送并读取目标回合，检查回合内容及来源，并核对
CLI 未创建 MCP 注册或其它 app-server 实例。等待和读回均使用同一 daemon。
