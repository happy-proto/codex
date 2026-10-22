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
| 0007 | [容量错误无限退避重试](0007-capacity-retries.md) |
| 0008 | [已打开会话的名称同步](0008-thread-name-refresh.md) |
| 0009 | [回合与近期平均生成速度](0009-generation-speed.md) |
| 0010 | [默认使用开放的浏览器与电脑操作工具](0010-open-automation.md) |
| 0011 | [WebSocket 连接主动轮转](0011-websocket-rotation.md) |
| 0012 | [后台压缩失败后的接收器清理](0012-async-compaction-closed-receiver.md)（alpha.8 已吸收） |
| 0013 | [发布版本与编译缓存分离](0013-build-cache-stability.md) |
| 0014 | [会话 Skills 自动刷新保持输入响应](0014-session-skills-refresh.md) |
| 0015 | [ts-rs 宏输出与编译元数据稳定性](0015-ts-rs-metadata-stability.md) |
| 0016 | [异步压缩生命周期诊断](0016-async-compaction-diagnostics.md) |
| 0017 | [后台压缩失败后的退避重试](0017-async-compaction-retry.md) |
| 0018 | [会话动态补全](0018-session-completion.md) |
| 0019 | [会话列表只展示当前 rollout](0019-current-rollout-listing.md) |
| 0020 | [OpenAI 模型请求专用代理](0020-openai-proxy.md) |

维护规则见 [AGENTS.md](../../AGENTS.md)，操作方法见 [维护 skill](../../.agents/skills/codex-fork-maintenance/SKILL.md)。
修改已有改造时更新原记录，新改造使用下一个编号；编号不重用，不对应 PR 或固定 commit SHA。
上游吸收或删除改造时保留原文件，更新状态、吸收版本和残留差异。
历史 PR 链接仅供追溯讨论，日常维护直接在 `main` 完成。
