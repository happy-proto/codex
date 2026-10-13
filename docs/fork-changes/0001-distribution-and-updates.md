# 0001 独立分发、更新和安装

状态：维护中。

## 为什么改

个人 fork 需要发布包含自有功能的完整 CLI，不能通过官方更新渠道覆盖为上游原版。
安装、缓存、版本号和崩溃后的锁释放必须支持长期更新。

## 如何改

`main` 保存选定上游 alpha 的原始提交，`fork` 直接维护全部改造。
`fork.yml` 并行执行 Linux Rust 测试、Linux AMD64 和 ARM macOS 完整包构建，通过后自动发布。
版本使用上游 alpha 加 `.fork.N`，不覆盖已发布版本；平台包和 SHA-256 由 `fork-release.json` 描述。
安装器校验下载、保留上一包供回退；用系统文件锁避免崩溃遗留锁阻断更新。
启动检查只显示一次后台提示，不自动安装；`codex update` 主动更新，daemon 默认关闭自动升级。
Rust 编译使用 mbx 对象缓存，下载缓存独立维护。

## 入口和验证

- [发布工作流](../../.github/workflows/fork.yml)、[发布脚本](../../scripts/fork/release.py) 与 [脚本测试](../../scripts/fork/test_release.py)。
- [安装器](../../scripts/fork/install.sh)、[终端产物验收](../../scripts/fork/test_terminal_cwd.py)。
- [更新入口](../../codex-rs/tui/src/update_action.rs)、[更新检测](../../codex-rs/tui/src/updates_cache.rs)、[daemon 设置](../../codex-rs/app-server-daemon/src/settings.rs)。

## 兼容和上游同步

**Breaking change**：新包采用 tar.xz，下载脚本须读取清单而不是拼 tar.gz 文件名；历史 gzip 包仍可安装。
启动时不再弹更新选择框，使用 `codex update`；需要 daemon 自动更新时显式开启设置。
同步时核对上游 package builder、资源布局、签名和更新入口。macOS 包为 ad-hoc 签名，Linux 包依赖 glibc。
验证必须区分 CI、发布产物和本机实际安装，不能仅凭版本输出判断完整包可用。
历史设计讨论：[PR #1](https://github.com/happy-proto/codex/pull/1)。
