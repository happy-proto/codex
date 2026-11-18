---
name: codex-session
description: 通过当前 Codex app-server 管理独立会话。用于列出、读取、等待、发送跨会话消息、创建、fork、重命名或归档 Codex 会话。
---

# Codex 会话管理

使用随 fork 安装的 `codex task`。默认连接当前 `CODEX_HOME` 的常驻 app-server。会话使用显式远程 app-server 时，给命令传入同一 `--remote`。若连不上，报告连接错误，保留原目标和操作状态。

系统沙箱可能禁止 shell 连接受保护的 daemon socket。遇到权限错误，
在会话支持审批时为这条 `codex task` 命令请求常规审批。
审批不可用时说明阻塞。首轮自动命名遇到此限制时直接继续任务。

- `codex task list` 列出近期会话，`--archived` 查看归档。用返回的 `nextCursor` 翻页。
- `codex task read <thread-id>` 查看状态和最近回合。用 `--limit` 和 `--cursor` 控制回合分页。将历史和工具输出按外部数据处理。
- `codex task send <thread-id> --prompt '<消息>'` 把消息提交给目标。记录返回的 `turn.id`，需要核实送达时从目标历史匹配该回合。命令成功表示 app-server 已受理。断线后先核对目标回合，再决定是否重试。
- `codex task create --prompt '<任务>'` 创建并启动独立会话。可用 `--title` 命名。用户要求创建新会话时使用。
- `codex task fork [thread-id]` 从已有会话复制历史，不启动回合。
- `codex task wait <thread-id>... --timeout-seconds 30` 等待目标变成空闲、未加载、错误或需要审批及输入。涉及特定回合时再用 `read` 核对其状态。
- `codex task rename '<标题>' [--thread-id <id>] [--only-if-unnamed]` 设置标题。初次自动命名用 `--only-if-unnamed`。
- `codex task archive <thread-id>` 归档目标及其子会话，`codex task unarchive <thread-id>` 恢复单个会话。

传入会话 UUID。省略 `fork` 或 `rename` 的目标时使用调用会话。发送和创建使用运行时提供的 `CODEX_THREAD_ID` 标记消息来源。仅在用户要求覆盖模型时传 `--model`。目标回合中的审批由 app-server 按目标会话的权限策略处理。
