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
受限 shell 连接受保护 socket 时沿用现有命令审批流程。
跨会话发送恢复目标并提交带来源的回合，返回服务端受理的回合 ID。
创建会话继承源会话的权限配置和审批策略。目标审批由 app-server 处理。
连接丢失后先检查目标回合再决定是否重试。摘要读取与等待复用 TUI 任务工具的
分页、输出边界、状态判断和游标。worktree 创建复用现有管理器、任务附件与本地执行环境检查。
`codex task worktree-create` 为当前会话创建并登记 checkout，使用命令执行器跟踪运行状态。
`codex task create --worktree` 创建 checkout 后在其中启动独立会话，并绑定其 worktree 附件。
两条路径均返回可用路径或登记失败时的恢复信息。

- [内置 skill](../../codex-rs/skills/src/assets/fork-native/codex-session/SKILL.md)
- [CLI 操作实现](../../codex-rs/tui/src/session_task_commands.rs)
- [旧 transport 开关](../../codex-rs/tui/src/app/startup.rs)
- [上游同步检查](../../.agents/skills/codex-fork-maintenance/references/upstream-sync.md#内置会话管理-skill-的发布门槛)

验收在隔离 daemon 中创建两个已持久化的会话，退出所有 TUI 后用
`codex task send` 发送并读取目标回合，检查回合内容及来源，并核对
CLI 未创建 MCP 注册或其它 app-server 实例。等待和读回均使用同一 daemon。
CI 还验证两种 worktree 创建命令的参数，以及实际 checkout、任务附件登记、权限与重新连接后的读取。
