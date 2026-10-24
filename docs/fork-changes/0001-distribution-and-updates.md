# 0001 独立分发、更新和安装

状态：维护中。

## 为什么改

个人 fork 需要发布包含自有功能的完整 CLI，不能通过官方更新渠道覆盖为上游原版。
安装、缓存、版本号和崩溃后的锁释放必须支持长期更新。

## 如何改

`main` 是唯一长期分支，直接维护全部改造；`scripts/fork/upstream-tag` 记录上游基线，定期 rebase 到新 alpha。
`fork.yml` 并行执行 Linux Rust 测试、Linux AMD64 和 ARM macOS 完整包构建，通过后自动发布。
发布、观察和快照更新等 Python CLI 通过 uv 的 PEP 723 入口运行，使用 Typer 定义参数和帮助；
最低 Python 版本为 3.15，CI 显式安装 uv 与 Python 3.15，构建子进程沿用同一解释器。
脚本测试通过 `scripts/fork/run_tests.py` 在声明的依赖环境中运行；普通被导入模块和测试不单独声明脚本环境。
**Breaking change：维护工具不再支持 Python 3.14 及更早版本的裸 `python3` 执行。**
维护者安装 uv 后直接执行 `scripts/fork/<工具>.py`，或使用 `uv run --script scripts/fork/<工具>.py`；
最终用户的 Shell 安装入口及其系统依赖不变，不要求安装 uv 或 Python 3.15。
macOS 构建显式使用 `macos-latest`，跟随 GitHub 当前稳定 runner 镜像；目标仍为
`aarch64-apple-darwin`。镜像升级后的完整包由 CI 原生验收，性能对照须区分实际镜像，
不把不同系统的耗时直接归因于缓存优化。标签对应的系统与架构见
[GitHub runner 映像说明](https://github.com/actions/runner-images#available-images)。
版本使用上游 alpha 加 `.fork.N`，不覆盖已发布版本；平台包和 SHA-256 由 `fork-release.json` 描述。
发布后仅按发布时间保留最新 10 个已发布版本及其 tag，不设时间窗口。
draft 不占名额；清理遗留 draft 和没有 Release 的孤立 fork tag，不影响上游 tag。
清理脚本默认只预览，`prune --apply` 才删除；自动发布完成后执行清理。
发布版本写入 `codex-package.json`，CLI、TUI、exec 展示和 daemon 身份在运行时读取。
发布包使用多线程 xz 等级 8，保留 tar.xz 格式和签名后的完整资源；在压缩耗时与下载
体积之间取平衡。调整等级前用真实平台包的相同 tar 字节流做对照，校验无损往返，
再由 CI 验证包内内容、可执行权限、原生运行与安装契约。
归档显式给 xz 50% 的物理内存预算，在线程数自动选择时避免默认软上限过早减少线程；
该预算只影响发布时的压缩，不改变 CLI 运行时设置。`-T0` 不保证用满所有 CPU，
对照时记录 `xz --info-memory`、实际线程数与 swap。调整预算时固定同一 tar 输入做
前后对照，核对 xz 输出摘要，确认压缩内容没有改变；不同架构的包不视为相同输入。
内部 crate 保留所选上游 alpha 的版本，不随 `.fork.N` 重写；源码、依赖和编译参数仍参与缓存校验。
版本解析、commit 注入及防回退约束见 [发布版本与编译缓存分离](0013-build-cache-stability.md)。
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
- [脚本测试入口](../../scripts/fork/run_tests.py) 与 [CLI 契约测试](../../scripts/fork/test_cli.py)：
  覆盖直接执行与 uv 脚本模式、帮助中的参数与可选值、非法参数、版本输出及命令退出码；
  [观察器测试](../../scripts/fork/test_build_observer.py) 验证经过 uv 启动后的信号转发与子进程回收。
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
