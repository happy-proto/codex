# 0033 Rex 终端程序恢复

交互式 `codex` 支持 Rex 的 [OSC 7577 0.5](https://www.superlogical.com/rex/docs/build/program-resume)。
Rex 可以在重启服务器后以 `codex resume <UUID>` 恢复 pane 的主会话。

- CLI 在创建线程前取走 `OSC7577_TOKEN`，仅在进程内交给 TUI。子进程环境、日志以及 OSC 报文均不携带 token。
- TUI 在主会话已有本机 rollout 时上报恢复命令和目录。切换到不可恢复的目标会撤销旧记录。用户明确退出时撤销记录，PTY 输入断开和终止信号保留记录。从 job control 挂起回到前台时重发。
- 上报形状符合 Rex 内置规则，命令以裸名称 `codex` 启动，目录由独立的 `cwd` 字段提供。只在当前 PATH 的 `codex` 指向当前运行的 CLI 时登记。恢复时按新 shell 的 PATH 查找 `codex`，并采用正常的本机后端选择。自定义 `CODEX_HOME`、显式后端、远端与 Cloud 会话以及无法重建的启动配置暂不登记。

实现入口是 `codex-rs/cli/src/main.rs`、`codex-rs/tui/src/program_resume.rs` 和 TUI 主事件循环。
保持会话格式和现有 resume CLI 兼容。发布 CI 运行 `program_resume` 模块测试。更新时检查参数逐项编码、超限后撤销旧记录、主动退出和 PTY 断开的区别。
