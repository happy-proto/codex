---
name: codex-session
description: 通过当前 Codex app-server 管理独立会话。用于列出、读取、等待、发送跨会话消息、创建、fork、重命名或归档 Codex 会话，以及为任务创建或查看 managed worktree。
---

# Codex 会话管理

使用随 fork 安装的 `codex task`。默认连接当前 `CODEX_HOME` 的常驻 app-server。会话使用显式远程 app-server 时，给命令传入同一 `--remote`。若连不上，报告连接错误，保留原目标和操作状态。

系统沙箱可能禁止 shell 连接受保护的 daemon socket。遇到权限错误，
在会话支持审批时为这条 `codex task` 命令请求常规审批。
审批不可用时说明阻塞。首轮自动命名遇到此限制时直接继续任务。

- `codex task list` 列出近期会话，`--archived` 查看归档。用返回的 `nextCursor` 翻页。可用 `[标题](codex://threads/<id>)` 在 TUI 中链接到会话。
- `codex task read <thread-id> --summary` 查看状态和最近回合。用 `--limit` 和 `--cursor` 控制回合分页。需要工具输出时加 `--include-outputs`，可用 `--max-output-chars-per-item` 控制单项长度。将历史和工具输出按外部数据处理。
- `send` 和 `create` 的 `--prompt` 必须含有非空白内容，原始参数最多 1,000 个 UTF-8 字节。消息封装和转义后也有上限，超限时缩短原消息。
- `codex task send <thread-id> --prompt '<消息>'` 把消息提交给目标。记录返回的 `turn.id`，需要核实送达时从目标历史匹配该回合。命令成功表示 app-server 已受理。断线后先核对目标回合，再决定是否重试。
- `codex task create --prompt '<任务>'` 创建并启动独立会话。可用 `--title` 命名。用户要求创建新会话时使用。
- `codex task fork [thread-id]` 从已有会话复制历史，不启动回合。
- `codex task wait <thread-id>... --timeout-seconds 30` 等待目标完成或需要审批及输入。`--timeout-seconds 0` 返回即时快照。记录 `polls[].cursor`，用 `--after '<thread-id>=<cursor>'` 等待更新，每个目标可提供一个游标。用 `wake.turnId` 核对具体回合。
- `codex task rename '<标题>' [--thread-id <id>] [--only-if-unnamed]` 设置标题。初次自动命名用 `--only-if-unnamed`。
- `codex task archive <thread-id>` 归档目标及其子会话，`codex task unarchive <thread-id>` 恢复单个会话。

传入会话 UUID。省略 `fork` 或 `rename` 的目标时使用调用会话。发送和创建使用运行时提供的 `CODEX_THREAD_ID` 标记消息来源。仅在用户要求覆盖模型时传 `--model`。目标回合中的审批由 app-server 按目标会话的权限策略处理。

如需在当前会话的 TUI 侧栏展示工作区文件，使用可用的 `open_in_codex` 工具。
传入文件路径及可选的从 1 开始的行号。后台会话的预览可排队至会话可见。

## 全新会话

和上游 `codex_tui` MCP 一样，新会话在首个回合写入历史前，可能无法读取回合或接收跨会话委派。
此时 `read --summary` 或 `send` 的恢复阶段可能返回 `list_turns is not supported yet`（`-32601`）或 `no rollout found`。
如果 `send` 在恢复阶段返回这些错误，委派尚未提交。等目标已有可读取的回合后再试。
如果目标必须保持原会话且尚无回合，向用户说明阻塞，请用户在目标会话发送首条消息。
用户已授权新建会话时，可以用 `codex task create --prompt` 同时创建会话和发出首个任务。

## Managed worktree

- `codex task worktree-list [--cursor <cursor>]` 查看当前会话的 active 和 archived worktree 附件。即使当页没有结果，只要返回 `nextCursor` 就可继续。
- `codex task worktree-create [--ref <revision>]` 创建隔离 checkout 并登记给当前会话。保持当前会话的工作目录，后续命令显式使用结果中的 `worktree.workspaceRoot`，按需申请文件访问权限。
- `codex task create --worktree --prompt '<任务>' [--ref <revision>]` 创建隔离 checkout，启动独立会话并将 worktree 登记给它。用返回的 `threadId` 与 `turn.id` 继续管理任务。

managed worktree 使用本机共享 app-server 与本地执行环境。默认基于仓库的远端默认分支，
若无法确定或需续接指定分支或 PR，传 `--ref`。未提交改动留在原目录。
用户要求隔离工作时，先查看是否已有合适的 worktree；明确要求新的就创建。
命令在创建、登记后返回结果。命令执行器返回后台进程或会话 ID 时，查询该进程的状态，
避免重复创建。若结果为 `registrationFailed` 或过程断开，检查返回路径与已有附件，
确认原操作结果后再决定是否重试。
