# 0033 Rex 终端剪贴板路由

状态：维护中。

## 动机与行为

Rex 会话可在 Codex 启动后接入其它电脑的客户端，启动时的客户端探测无法决定后续复制目标。
检测到 `REX_SESSION` 时，TUI 每次复制都只发送 OSC 52，不初始化或写入宿主机原生剪贴板，
也不调用 Rex CLI 或探测客户端列表。本地、远程和混合连接采用相同规则。

该判断独立于 SSH，不伪造 `SSH_TTY`，不改变 SSH 检测、粘贴、登录或本地文件链接。
非 Rex 环境保留原有原生剪贴板、WSL 和终端转发规则。

复制沿用现有 worker、取消检查和终端写入流程；嵌套 tmux/screen 时继续使用 OSC 52 透传封装，
不调用 tmux 复制命令。空内容报错，原始 UTF-8 内容上限为 100,000 字节，终端写入失败继续报错。
OSC 52 只携带纯文本，即使复制 Markdown 也不向宿主机写入 HTML。
返回成功只表示请求已发送，继续使用未确认的复制状态，不宣称客户端剪贴板已更新。
本改造仅控制 TUI 复制，不约束模型通过工具执行 `pbcopy` 等程序。

## 入口与验证

- `codex-rs/tui/src/clipboard_copy.rs`：独立 Rex 状态、原生初始化跳过与终端优先路由。
- `codex-rs/tui/src/clipboard_copy/routing_tests.rs`：隔离后端验证重复复制、无 SSH 标记、
  原生剪贴板可用仍不写入、WSL/tmux 组合、空内容、大小限制和终端错误；原有非 Rex 回归继续运行。
- `.github/workflows/fork.yml`：持续运行 `clipboard_copy` 测试组。

真实验收需区分 OSC 52 发送与客户端实际交付，并覆盖本地启动后远程客户端再接入。
关联需求：[upkeep PR #80](https://github.com/DCjanus/upkeep/pull/80)。

## 上游同步

关注原生剪贴板初始化位置、复制 worker 的终端序列与请求状态；Rex 分支必须先于原生初始化和写入。
保留 SSH 检测的原有定义，不能把 Rex 加入原生复制之后的补发条件。
若上游提供等效的 Rex 专用路径，移除重复实现并保留本记录。
