# 发布、更新和安装

`fork` 的每次更新在验证通过后自动发布，支持 `aarch64-apple-darwin` 和 `x86_64-unknown-linux-gnu`。
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
通用脚本检查和 Rust 测试使用 Linux x64 runner；完整包通过平台 matrix 并行构建，
Linux x64 和 ARM macOS runner 分别验证本机安装契约和真实 CLI；仅 macOS 签名。
分析耗时时读取各 job 的日志和 `fork-build-timings-*`、`fork-test-timings-*` artifact，
用 Cargo 报告区分 crate 编译和并发等待，不把日志末尾的整段耗时直接归为链接。

Rust 编译由 mbx 1.23.0 包装 Cargo，使用 GitHub Actions 的 `objects` 缓存模式。
只导出本次构建使用或生成的对象；Cargo registry 和 Git 依赖下载由独立 rust-cache 保存，
该下载缓存关闭 target 和 bin 缓存，不与 mbx 重复保存编译产物。
构建与测试按平台和 profile 使用独立 cache generation，避免并行 job 争用同一条目。
设置 `MBX_TARGET_VIEWS=0` 保留上游 package builder 和耗时报告使用的 workspace target 路径。
V8、rg 继续由上游下载器校验和获取，不额外缓存下载资源。
使用 mbx Action 默认的对象库布局，避免隔离模式清理只读 build-script 输出时的权限错误。
每次导出只包含该 job 的构建闭包，不归档整个历史对象库；runner 在 job 结束后销毁。

先在默认分支 `fork` 预热；其它分支可读取默认分支缓存，默认分支不能反向读取功能分支的私有缓存。
因此各功能 PR 使用独立的 `fork-pr.yml`，只定义轻量脚本检查，更新后取消旧检查。
`fork.yml` 只接受集成顶部的 push 和手动触发，避免 PR 创建注定跳过的重型 Job；
用 workflow_dispatch 按需验证指定层。
手动实验默认不发布，`publish=true` 才请求发布 fork；受信任的手动完整验证允许保存缓存。
只需验证 TUI 回归时，手动指定 `tui_test_filter`；该模式只运行匹配的 TUI 库测试，
跳过产物构建、其它 Rust 测试及发布，不受 `publish` 输入影响。
手动实验和自动发布使用不同并发组，避免耗时实验占用自动发布队列。
比较迁移前完整版本、首次预热和热缓存完整版本，分别记录构建、测试 job 的执行时间、
缓存恢复/保存时间和排队时间；读取 `fork-*-mbx-*` 统计及 Cargo 耗时报告。
不能用冷缓存一次运行或单独的编译加速数字判断最终收益；持续超过 10% 的耗时增长应复查再接入。

修改发布流程时检查：版本与选定 alpha 一致、源码属于完整 fork 分支、签名后的资源摘要正确、
产物先上传验证再更新清单、新 fork 序号可被识别、下载损坏不会切换 current、并发发布不会覆盖已发布版本或不同源码的 draft。
不要仅测 `codex --version`：还要验证 package 资源、code-mode host、app-server 启动及实际更新行为。

更新只提示，由用户执行 `codex update`。不自动更新到官方源，不启用后台自动升级。
安装保留此前包，使用原有 `CODEX_HOME` 和 `codex` 命令；替换日常 CLI 前先在隔离目录验证完整包。
回退时选择以前的包或重装官方 CLI，保留用户配置、会话和凭据。不得在输出中暴露凭据。

平台及路径的具体接口由 README、发布工作流和 `scripts/fork/` 的代码定义；先读源码再操作。
每次交付明确区分本地验证、GitHub CI、Release 产物及本机实际运行版本。
