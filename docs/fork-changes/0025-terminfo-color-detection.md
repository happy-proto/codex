# 0025 terminfo 颜色能力回退

状态：维护中。

## 动机

SSH 通常只随伪终端请求传递 `TERM`，不转发 `COLORTERM`。
远端即使安装了声明真彩色的 `xterm-ghostty` terminfo，原有环境变量检测仍可能只识别出
16 色，导致用户消息和输入框背景消失。上游问题见 [#23677](https://github.com/openai/codex/issues/23677)。

## 实现

Unix TUI 在环境变量检测不足时，用纯 Rust `terminfo` 库读取本机数据库，遵循该库的
`TERMINFO`、用户目录、`TERMINFO_DIRS` 和系统目录查找规则，不调用外部命令或链接 ncurses。
`Tc`、布尔或正整数 `RGB`、成对且非空的 `setrgbf` / `setrgbb`，以及至少 2²⁴ 色的声明
作为真彩色依据；普通 `colors#256` 只提升到 256 色，不推断真彩色。

只提升现有颜色级别，不降级。保留显式 `FORCE_COLOR`、`CLICOLOR_FORCE`、`NO_COLOR`、
`TERM=dumb` 与重定向输出的原有判断；数据库缺失或解析失败时沿用环境变量结果。
结果按进程缓存，渲染期间不重复读取数据库。Windows 保留原有检测。

这只检测颜色能力，当前主题的前景与背景颜色仍由已有 OSC 10/11 探测获取。

## 代码入口

- [颜色级别公共入口](../../codex-rs/tui/src/terminal_palette.rs)
- [terminfo 检测](../../codex-rs/tui/src/terminal_palette/terminfo_color.rs)
- [子进程与伪终端回归测试](../../codex-rs/tui/src/terminal_palette/terminfo_tests.rs)
- [fork CI](../../.github/workflows/fork.yml)

## 验证与维护

`terminal_palette` 测试组覆盖仅有 `TERM` 的真彩色声明、不同能力表示、256 色与真彩色区分、
缺失或损坏条目、显式颜色设置和非终端输出。回归测试使用固定编译条目和独立子进程，
不修改测试进程环境，也不依赖测试主机安装 Ghostty；条目来源与重新生成命令保存在相邻 `.src` 中。
CI 的 release 条件测试持续执行该组。

同步上游时检查 `supports-color` 及颜色公共入口是否已支持 terminfo，避免重复检测；
保留显式设置优先级和进程缓存，不以终端品牌替代数据库能力。
