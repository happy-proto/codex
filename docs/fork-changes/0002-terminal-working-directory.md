# 0002 OSC 7 终端工作目录

## 为什么改

终端需要知道当前本地会话的目录，才能在同目录打开标签页、分屏或正确解析相对路径链接。

## 如何改

在启动、恢复、会话切换和完成 worktree 切换后，渲染前报告当前目录。
使用 `file://localhost/...` 的 OSC 7 格式，按字节编码路径；目录不变时不重复输出。
仅覆盖本地会话，远程环境与非终端输出不发送。

## 入口和验证

- [编码与输出](../../codex-rs/tui/src/terminal_working_directory.rs)、[ChatWidget 生命周期](../../codex-rs/tui/src/chatwidget/working_directory.rs)。
- [产物 PTY 验收](../../scripts/fork/test_terminal_cwd.py)：真实 CLI 启动与 resume。

## 上游同步

关注会话 attach、cwd 和 worktree 生命周期；不能只在首次启动发送。
保留 Unicode、空格、非 UTF-8 名称和控制字符的编码测试，避免把目录输出写到重定向内容里。
历史讨论：[PR #3](https://github.com/happy-proto/codex/pull/3)。
