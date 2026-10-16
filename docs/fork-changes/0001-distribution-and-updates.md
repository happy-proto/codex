# 0001 独立分发、更新和安装

状态：维护中。

## 为什么改

个人 fork 需要发布包含自有功能的完整 CLI，不能通过官方更新渠道覆盖为上游原版。
安装、缓存、版本号和崩溃后的锁释放必须支持长期更新。

## 如何改

`main` 是唯一长期分支，直接维护全部改造；`scripts/fork/upstream-tag` 记录上游基线，定期 rebase 到新 alpha。
`fork.yml` 并行执行 Linux Rust 测试、Linux AMD64 和 ARM macOS 完整包构建，通过后自动发布。
版本使用上游 alpha 加 `.fork.N`，不覆盖已发布版本；平台包和 SHA-256 由 `fork-release.json` 描述。
发布版本写入 `codex-package.json`，CLI、TUI、exec 展示和 daemon 身份在运行时读取，
不再通过 `cargo set-version` 改动整个 workspace。内部 crate 保留所选上游 alpha 的版本，
内部协议和遥测中原有的 Cargo 版本字段也保留该基线；源码、依赖和编译参数仍参与 sccache 缓存校验。
同一 alpha 的 `.fork.N` 变化不再单独使全部内部 crate 失效；同步新的 alpha 或改变其它编译输入时仍可能重编译。
未置于完整包中的本地可执行文件回退到 Cargo 基线版本；Git commit 仍只注入最终可执行文件。
安装器校验下载、保留上一包供回退；用系统文件锁避免崩溃遗留锁阻断更新。
启动检查只显示一次后台提示，不自动安装；`codex update` 主动更新，daemon 默认关闭自动升级。
Rust 编译由 Cargo 通过 sccache 的 GitHub Actions 后端缓存，按编译结果独立保存，关闭 incremental；
构建与测试共享缓存命名空间，下载缓存独立维护，不保存整个 target 或每个 job 的构建闭包。
两类缓存仅允许默认分支写入；其它分支的手动运行只恢复缓存，避免实验构建挤占正式缓存。
保留编译缓存的 JSON 统计和 Cargo 耗时报告；实际空间与耗时收益需通过 CI 的冷、热缓存运行验证。
自有发布工作流使用可读的 action 版本引用，由 `.github/workflows/actions.lock` 锁定实际提交与传递依赖；
使用 `gh actions-lock .github/workflows/fork.yml --no-migrate-local-actions` 生成，配合 `--verify` 校验，只纳入自有工作流。
发布前校验记录的上游 tag 是源码祖先，两次核对默认分支头，避免旧构建覆盖新版本。

## 入口和验证

- [发布工作流](../../.github/workflows/fork.yml)、[发布脚本](../../scripts/fork/release.py) 与 [脚本测试](../../scripts/fork/test_release.py)。
- [版本解析](../../codex-rs/build-info/src/lib.rs)、[包版本回归测试](../../scripts/fork/test_package_version.py)：
  将同一个未修改的 binary 放入不同版本的包，验证 `--version` 随清单变化；两个平台的原生产物都执行此检查。
- [安装器](../../scripts/fork/install.sh)、[终端产物验收](../../scripts/fork/test_terminal_cwd.py)。
- [更新入口](../../codex-rs/tui/src/update_action.rs)、[更新检测](../../codex-rs/tui/src/updates_cache.rs)、[daemon 设置](../../codex-rs/app-server-daemon/src/settings.rs)。

## 兼容和上游同步

**Breaking change**：新包采用 tar.xz，下载脚本须读取清单而不是拼 tar.gz 文件名；历史 gzip 包仍可安装。
启动时不再弹更新选择框，使用 `codex update`；需要 daemon 自动更新时显式开启设置。
默认分支迁移到 `main` 后删除旧 `fork` 分支；旧版 CLI 内置的分支安装 URL 随之失效，
需使用 README 中的 `main` 安装命令更新一次，之后继续使用 `codex update`。
根 README 只保留使用入口，[上游英文说明](../upstream-readme.md) 独立保存；同步上游时更新该说明并核对相对链接。
同步时核对上游 package builder、资源布局、签名和更新入口。macOS 包为 ad-hoc 签名，Linux 包依赖 glibc。
验证必须区分 CI、发布产物和本机实际安装，不能仅凭版本输出判断完整包可用。
历史设计讨论：[PR #1](https://github.com/happy-proto/codex/pull/1)。
