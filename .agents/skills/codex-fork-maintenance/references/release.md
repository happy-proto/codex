# 发布、更新和安装

`fork` 的每次更新在验证通过后自动发布，首期仅 `aarch64-apple-darwin`。
版本沿用选定上游 alpha 加 `.fork`，无 fork 序号。同一 alpha 下重发更新同名 Release。
软件版本不能唯一标识构建：用源 SHA、包摘要和发布清单识别实际构建。

构建复用上游完整 package builder，包含 CLI、code-mode host、rg 和 zsh 等必要资源。
macOS 使用 ad-hoc 签名并保留上游 entitlements；不配置官方 Azure、R2、npm、WinGet 或网站发布设施。
下载 CI/Release 产物做隔离安装与轻量运行验收。
标准 macOS runner 使用 release 优化、关闭跨 crate Thin LTO；`lto=false` 仍可能包含单 crate 优化。
测试与产物构建并行；测试保留 release 条件编译及关闭 debug assertions，禁用测试优化以控制编译成本。
通用脚本检查和 Rust 测试使用 Linux x64 runner；只有 Apple Silicon 完整包的构建、签名、
macOS 安装契约和真实 CLI 验收使用 ARM macOS runner，避免通用测试受其容量队列影响。
分析耗时时读取各 job 的日志和 `fork-build-timings-*`、`fork-test-timings-*` artifact，
用 Cargo 报告区分 crate 编译和并发等待，不把日志末尾的整段耗时直接归为链接。

Rust 编译由 mbx 1.22.0 包装 Cargo，使用 GitHub Actions 的 `objects` 缓存模式。
只导出本次构建使用或生成的对象；Cargo registry 和 Git 依赖下载由独立 rust-cache 保存，
该下载缓存关闭 target 和 bin 缓存，不与 mbx 重复保存编译产物。
构建与测试按平台和 profile 使用独立 cache generation，避免并行 job 争用同一条目。
设置 `MBX_TARGET_VIEWS=0` 保留上游 package builder 和耗时报告使用的 workspace target 路径。
V8、rg、zsh 继续由上游下载器校验和获取，不额外缓存下载资源。
使用 mbx Action 默认的对象库布局，避免隔离模式清理只读 build-script 输出时的权限错误。
每次导出只包含该 job 的构建闭包，不归档整个历史对象库；runner 在 job 结束后销毁。

先在默认分支 `fork` 预热；其它分支可读取默认分支缓存，默认分支不能反向读取功能分支的私有缓存。
因此维持各功能 PR 的轻量检查，只在集成顶部自动执行重型构建；用 workflow_dispatch 按需验证指定层。
手动实验默认不发布，`publish=true` 才请求发布 fork；受信任的手动完整验证允许保存缓存。
只需验证 TUI 回归时，手动指定 `tui_test_filter`；该模式只运行匹配的 TUI 库测试，
跳过产物构建、其它 Rust 测试及发布，不受 `publish` 输入影响。
手动实验和自动发布使用不同并发组，避免耗时实验占用自动发布队列。
比较迁移前完整版本、首次预热和热缓存完整版本，分别记录构建、测试 job 的执行时间、
缓存恢复/保存时间和排队时间；读取 `fork-*-mbx-*` 统计及 Cargo 耗时报告。
不能用冷缓存一次运行或单独的编译加速数字判断最终收益；持续超过 10% 的耗时增长应复查再接入。

修改发布流程时检查：版本与选定 alpha 一致、源码属于完整 fork 分支、签名后的资源摘要正确、
产物先上传验证再更新清单、同版本更新可被识别、下载损坏不会切换 current、并发发布不会让旧构建覆盖新构建。
不要仅测 `codex --version`：还要验证 package 资源、code-mode host、app-server 启动及实际更新行为。

更新只提示，由用户执行 `codex update`。不自动更新到官方源，不启用后台自动升级。
安装保留此前包，使用原有 `CODEX_HOME` 和 `codex` 命令；替换日常 CLI 前先在隔离目录验证完整包。
回退时选择以前的包或重装官方 CLI，保留用户配置、会话和凭据。不得在输出中暴露凭据。

平台及路径的具体接口由 README、发布工作流和 `scripts/fork/` 的代码定义；先读源码再操作。
每次交付明确区分本地验证、GitHub CI、Release 产物及本机实际运行版本。
