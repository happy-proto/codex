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
| 0007 | [工作中显式 skill 正文注入](0007-steered-skills.md) |
| 0008 | [容量错误无限退避重试](0008-capacity-retries.md) |
| 0009 | [已打开会话的名称同步](0009-thread-name-refresh.md) |
| 0010 | [回合平均生成速度](0010-generation-speed.md) |
| 0011 | [默认使用开放的浏览器与电脑操作工具](0011-open-automation.md) |
| 0012 | [Responses Lite 请求来源字段兼容](0012-responses-lite-content-metadata.md) |
| 0013 | [WebSocket 连接主动轮转](0013-websocket-rotation.md) |

维护规则见 [AGENTS.md](../../AGENTS.md)，操作方法见 [维护 skill](../../.agents/skills/codex-fork-maintenance/SKILL.md)。
修改已有改造时更新原记录，新改造使用下一个编号；编号不重用，不对应 PR 或固定 commit SHA。
上游吸收或删除改造时保留原文件，更新状态、吸收版本和残留差异。
历史 PR 链接仅供追溯讨论，日常维护直接在 `main` 完成。
