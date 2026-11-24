# 0031 终端剪贴板路由

状态：维护中。

## 动机与行为

Rex 等持久会话可在 Codex 启动后接入其他客户端。先写宿主机剪贴板再转发给终端，
可能同时改写两端的剪贴板。TUI 沿用上游的会话和终端识别。检测到 tmux、SSH、Herdr 或 Rex 时，
每次复制只发送 OSC 52，跳过宿主机原生剪贴板和 WSL 剪贴板，也不调用 tmux 剪贴板命令。
其他会话优先尝试原生剪贴板或 WSL 的 Windows 剪贴板，失败后回退到 OSC 52。

复制沿用现有 worker、取消检查和终端写入流程。tmux 和 GNU screen 使用各自的透传封装，
能否交付取决于透传配置和客户端的 OSC 52 支持。空内容报错，原始 UTF-8 内容上限为
100,000 字节，终端写入失败继续报错。OSC 52 只携带纯文本，成功写出报告复制请求已发送。
本改造仅控制 TUI 复制，不约束模型通过工具执行 `pbcopy` 等程序。

## 入口与验证

- `codex-rs/tui/src/terminal_capabilities.rs` 将 tmux、SSH、Herdr 和 Rex 映射到同一复制策略。
- `codex-rs/tui/src/clipboard_copy.rs` 跳过 OSC 52 直发路径的原生初始化和写入。
- `codex-rs/tui/src/clipboard_copy/worker.rs` 根据 tmux 或 GNU screen 环境封装 OSC 52。
- `codex-rs/tui/src/clipboard_copy/routing_tests.rs` 验证重复发送、跳过原生及 WSL 写入、空内容、
  大小限制和终端错误。`.github/workflows/fork.yml` 持续运行 `clipboard_copy` 测试组。

真实验收需区分 OSC 52 发送与客户端实际交付，并覆盖本地启动后远程客户端再接入。
关联需求：[upkeep PR #80](https://github.com/DCjanus/upkeep/pull/80)。

## 上游同步

保留上游会话和终端识别，关注原生初始化、worker 透传封装和复制请求状态。
若上游提供等效的 OSC 52 直发路径，移除重复实现并保留本记录。
