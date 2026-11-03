# Fork 改造记录

`main` 是唯一长期分支，维护全部自有改动；`scripts/fork/upstream-tag` 记录选定上游 alpha。
实际差异以 `git diff "$(cat scripts/fork/upstream-tag)" HEAD` 为准；本目录解释为什么改、如何改和长期维护边界。
记录中的“维护中”表示当前代码仍有此改造，不替代最新 CI、Release 或安装验收。

| 编号 | 改造点 |
| --- | --- |
| 0001 | [独立分发、更新和安装](0001-distribution-and-updates.md) |
| 0002 | [OSC 7 终端工作目录](0002-terminal-working-directory.md) |
| 0003 | [中文 Markdown 加粗边界](0003-cjk-emphasis.md) |
| 0004 | [功能列表输出格式](0004-features-list.md) |
| 0005 | [MCP 列表展示](0005-mcp-list.md) |
| 0006 | [工作中显式 skill 正文注入](0006-steered-skills.md) |
| 0007 | [已打开会话的名称同步](0007-thread-name-refresh.md) |
| 0008 | [回合与近期平均生成速度](0008-generation-speed.md) |
| 0009 | [默认使用开放的浏览器与电脑操作工具](0009-open-automation.md) |
| 0010 | [WebSocket 连接主动轮转](0010-websocket-rotation.md) |
| 0011 | [发布版本与编译缓存分离](0011-build-cache-stability.md) |
| 0012 | [ts-rs 宏输出与编译元数据稳定性](0012-ts-rs-metadata-stability.md) |
| 0013 | [异步压缩生命周期诊断](0013-async-compaction-diagnostics.md) |
| 0014 | [后台压缩失败后的退避重试](0014-async-compaction-retry.md) |
| 0015 | [会话动态补全](0015-session-completion.md) |
| 0016 | [会话列表只展示当前 rollout](0016-current-rollout-listing.md) |
| 0017 | [OpenAI 模型请求专用代理](0017-openai-proxy.md) |
| 0018 | [每次重试等待提示](0018-advised-retry-visibility.md) |
| 0019 | [容量错误缺省等待重试](0019-overload-default-retry.md) |
| 0020 | [WebSocket 消息过大错误](0020-websocket-message-size.md) |
| 0021 | [daemon 关停拒绝后的 TUI 重连](0021-daemon-shutdown-recovery.md) |
| 0022 | [terminfo 颜色能力回退](0022-terminfo-color-detection.md) |
| 0023 | [通过 @ 选择插件提供的 skill](0023-at-plugin-skills.md) |
| 0024 | [首轮由 Agent 主动命名会话](0024-agent-thread-title.md) |
| 0025 | [分栏任务状态与共享底栏](0025-workspace-status-and-footer.md) |
| 0026 | [为所有模型开启输入框星光](0026-all-model-composer-stars.md) |
| 0027 | [按项忽略 Cloud config](0027-ignore-cloud-config.md) |
| 0031 | [内置会话管理 skill](0031-session-skill.md) |

维护规则见 [AGENTS.md](../../AGENTS.md)，操作方法见 [维护 skill](../../.agents/skills/codex-fork-maintenance/SKILL.md)。
同步上游按 [同步与发布](../../.agents/skills/codex-fork-maintenance/references/upstream-sync.md) 执行。
日常修改沿用已有编号，新改造使用下一个编号。完整历史整理按 [历史整理](../../.agents/skills/codex-fork-maintenance/references/history.md) 清理并重排。编号不对应 PR 或固定 commit SHA。
日常同步遇到上游吸收或删除的改造时保留原文件，更新状态、吸收版本和残留差异。
历史 PR 链接仅供追溯讨论，日常维护直接在 `main` 完成。
