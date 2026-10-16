# 发布、更新和安装

`main` 的每次更新在验证通过后自动发布，支持 `aarch64-apple-darwin` 和 `x86_64-unknown-linux-gnu`。
版本沿用选定上游 alpha 加 `.fork.N`，同一 alpha 独立递增，新 alpha 从 1 开始。
版本在脚本 job 统一选择，测试和两个并行构建 job 共享；发布 job 共用锁，构建与测试保持并行。
已发布版本不覆盖，失败构建允许序号空缺；并发构建发生序号冲突时拒绝覆盖，重新运行选择新序号。
Release 内使用固定平台包名，发布清单的 `packages` 记录各平台摘要；顶层包字段继续描述 macOS。
旧 `.fork` 安装首次手动执行 `codex update` 迁移。
发布后清理超过 7 天的 fork Release 和 tag，按发布时间最新三个已发布版本始终保留；
独立旧 tag 按 tagger 时间或轻量 tag 的提交时间清理，不影响本地安装回退。
使用 `release.py prune` 查看候选，`prune --apply` 执行清理。

构建复用上游完整 package builder，包含 CLI、code-mode host 和 rg 等必要资源。
从上游 `0.163.0-alpha.1` 起不再附带补丁 zsh，签名、验收和安装检查遵循当前上游布局。
macOS 使用 ad-hoc 签名并保留上游 entitlements；不配置官方 Azure、R2、npm、WinGet 或网站发布设施。
下载 CI/Release 产物做隔离安装与轻量运行验收。
标准 macOS runner 使用 release 优化、关闭跨 crate Thin LTO；`lto=false` 仍可能包含单 crate 优化。
测试与产物构建并行；测试保留 release 条件编译及关闭 debug assertions，禁用测试优化以控制编译成本。
Core 集成测试沿用上游 `ci-test` profile，保留测试所需的 debug assertions；
skill 集成测试串行运行，避免线程本地 tracing 捕获受并行用例干扰。
通用脚本检查和 Rust 测试使用 Linux x64 runner；完整包通过平台 matrix 并行构建，
Linux x64 和 ARM macOS runner 分别验证本机安装契约和真实 CLI；仅 macOS 签名。
分析耗时时读取各 job 的日志和 `fork-build-timings-*`、`fork-test-timings-*` artifact，
用 Cargo 报告区分 crate 编译和并发等待，不把日志末尾的整段耗时直接归为链接。

Rust 编译由 Cargo 执行，通过 `RUSTC_WRAPPER=sccache` 接入 sccache 0.18.0，
使用 GitHub Actions 远端缓存，按编译结果独立保存，不归档整个 target 或对象库。
关闭 Cargo incremental，避免编译请求不可缓存；沿用上游 package builder 和 workspace target 路径。
构建与测试共用 `SCCACHE_GHA_VERSION=fork-v1` 命名空间，
由 sccache 按编译器及编译输入区分缓存，允许相同输入复用，不再为每个 job 导出独立闭包。
仅默认分支使用 `READ_WRITE`；其它分支设置 `SCCACHE_GHA_RW_MODE=READ_ONLY`。
Cargo registry 和 Git 依赖下载继续由独立 rust-cache 保存，关闭 target 和 bin 缓存。
V8、rg 继续由上游下载器校验和获取，不额外缓存下载资源。
链接、bin、proc-macro 等不受 sccache Rust 缓存支持的请求仍正常编译；
不能将这些请求算作可缓存任务的 miss，具体限制见 [sccache Rust 文档](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)。
Action 在 job summary 输出统计，同时保存 `fork-*-sccache-*` JSON artifact，核对 hits、misses、
不可缓存原因和缓存读写错误。保持服务运行到 job 结束，避免长时间链接或测试后统计因空闲退出而丢失。

在默认分支 `main` 预热和保存正式缓存。
`fork.yml` 接受 `main` 的 push 和手动触发；提交直接在 `main` 维护，
没有功能 PR 专用工作流。需要快速反馈时用 workflow_dispatch 指定测试过滤条件。
手动实验默认不发布，`publish=true` 才请求发布 fork；仅默认分支的自动或手动运行允许保存编译及下载缓存，其它分支只恢复缓存。
只需验证 TUI 回归时，手动指定 `tui_test_filter`；该模式只运行匹配的 TUI 库测试，
跳过产物构建、其它 Rust 测试及发布，不受 `publish` 输入影响。
显式 skill 回归可通过 `core_test_filter` 定向运行，采用相同的跳过规则。
手动实验和自动发布使用不同并发组，避免耗时实验占用自动发布队列。
比较迁移前完整版本、首次预热和热缓存完整版本，分别记录构建、测试 job 的执行时间、
缓存读写时间和排队时间；读取 `fork-*-sccache-*` 统计及 Cargo 耗时报告。
远端空间占用需另外读取仓库 Actions cache 清单，不把单个 job 的编译统计当作总占用。
不能用冷缓存一次运行或单独的编译加速数字判断最终收益；持续超过 10% 的耗时增长应复查再接入。

修改发布流程时检查：版本与选定 alpha 一致、源码属于默认分支的完整历史且记录的上游 tag 是其祖先、签名后的资源摘要正确、
产物先上传验证再更新清单、新 fork 序号可被识别、下载损坏不会切换 current、并发发布不会覆盖已发布版本或不同源码的 draft。
不要仅测 `codex --version`：还要验证 package 资源、code-mode host、app-server 启动及实际更新行为。

更新只提示，由用户执行 `codex update`。不自动更新到官方源，不启用后台自动升级。
安装保留此前包，使用原有 `CODEX_HOME` 和 `codex` 命令；替换日常 CLI 前先在隔离目录验证完整包。
回退时选择以前的包或重装官方 CLI，保留用户配置、会话和凭据。不得在输出中暴露凭据。

平台及路径的具体接口由 README、发布工作流和 `scripts/fork/` 的代码定义；先读源码再操作。
每次交付明确区分本地验证、GitHub CI、Release 产物及本机实际运行版本。

## GitHub Actions 依赖锁定

`fork.yml` 的 `uses` 使用可读的版本或分支引用，实际提交及传递依赖由
`.github/workflows/actions.lock` 锁定；不要手工编辑 lockfile，也不在工作流里直接维护 SHA。
当前只将自有 `fork.yml` 纳入锁定，避免生成过程改写禁用的上游工作流。
使用 GitHub 官方 CLI 扩展生成和校验：

```sh
gh extension install github/gh-actions-lock
gh actions-lock .github/workflows/fork.yml --no-migrate-local-actions
gh actions-lock .github/workflows/fork.yml --verify
```

增加、删除或调整 action 引用后重新生成，审查 workflow 和 lockfile 的完整差异。
普通生成保留已有锁定；需要跟随可移动分支或版本标签更新时，审查上游变化后用同一作用域的 `--relock`。
生成时保留 `--no-migrate-local-actions`，否则扩展还会将不在本轮作用域内的本地 action 引用迁移到 `$/…`。
Rust toolchain action 的 `1.95.0` 是上游分支名，由 lockfile 固定实现；
更新 Rust 时同时核对 `codex-rs/rust-toolchain.toml` 与该引用，不将其当成 action 的发行版本。
原生 lockfile 当前为公开预览，行为以 [GitHub 文档](https://docs.github.com/en/actions/how-tos/secure-your-work/lock-workflow-dependencies)为准。
