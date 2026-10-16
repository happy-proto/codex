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

维护规则见 [AGENTS.md](../../AGENTS.md)，操作方法见 [维护 skill](../../.agents/skills/codex-fork-maintenance/SKILL.md)。
同步上游按 [同步与发布](../../.agents/skills/codex-fork-maintenance/references/upstream-sync.md) 执行。
日常修改沿用已有编号，新改造使用下一个编号。完整历史整理按 [历史整理](../../.agents/skills/codex-fork-maintenance/references/history.md) 清理并重排。编号不对应 PR 或固定 commit SHA。
日常同步遇到上游吸收或删除的改造时保留原文件，更新状态、吸收版本和残留差异。
历史 PR 链接仅供追溯讨论，日常维护直接在 `main` 完成。
