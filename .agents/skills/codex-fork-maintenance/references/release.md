# 发布、更新和安装

`fork` 的每次更新在验证通过后自动发布，首期仅 `aarch64-apple-darwin`。
版本沿用选定上游 alpha 加 `.fork`，无 fork 序号。同一 alpha 下重发更新同名 Release。
软件版本不能唯一标识构建：用源 SHA、包摘要和发布清单识别实际构建。

构建复用上游完整 package builder，包含 CLI、code-mode host、rg 和 zsh 等必要资源。
macOS 使用 ad-hoc 签名并保留上游 entitlements；不配置官方 Azure、R2、npm、WinGet 或网站发布设施。
下载 CI/Release 产物做隔离安装与轻量运行验收。
标准 macOS runner 使用 release 优化、关闭跨 crate Thin LTO；`lto=false` 仍可能包含单 crate 优化。
测试与产物构建并行；测试保留 release 条件编译及关闭 debug assertions，禁用测试优化以控制编译成本。
分析耗时时读取各 job 的日志和 `fork-build-timings-*`、`fork-test-timings-*` artifact，
用 Cargo 报告区分 crate 编译和并发等待，不把日志末尾的整段耗时直接归为链接。

测试编译默认使用 sccache 的 GitHub Actions 后端；产物编译只在手动实验中启用。
稳定版 0.18.0 在未命中时延迟 metadata 通知，削弱 Cargo 流水线；
等包含 [上游修复](https://github.com/mozilla/sccache/pull/2875) 的稳定版发布后再验证产物默认启用的收益。
在 rust-cache 恢复后才启用 wrapper，
避免改变已有依赖缓存的环境指纹。sccache 按源码和编译参数区分条目，不按 stack 分支拆分。
使用 `SCCACHE_IDLE_TIMEOUT=0`，避免长编译期间没有新请求时 daemon 退出、回退本地编译并丢失统计。
先在默认分支 `fork` 预热；其它分支可读取默认分支缓存，默认分支不能反向读取功能分支的私有缓存。
因此维持各功能 PR 的轻量检查，只在集成顶部自动执行重型构建；用 workflow_dispatch 按需验证指定层。
手动实验默认不发布，`use_sccache=false` 可做相同源码的对照，`publish=true` 才请求发布 fork。
手动实验和自动发布使用不同并发组，避免耗时实验占用自动发布队列。
比较同一提交的预热、热缓存及无 sccache 运行；同时读取 `fork-*-sccache-*` 统计与 Cargo 耗时报告，
区分缓存命中、不可缓存的最终 binary/test harness 和 runner 波动，不能仅凭总时长判断收益。

修改发布流程时检查：版本与选定 alpha 一致、源码属于完整 fork 分支、签名后的资源摘要正确、
产物先上传验证再更新清单、同版本更新可被识别、下载损坏不会切换 current、并发发布不会让旧构建覆盖新构建。
不要仅测 `codex --version`：还要验证 package 资源、code-mode host、app-server 启动及实际更新行为。

更新只提示，由用户执行 `codex update`。不自动更新到官方源，不启用后台自动升级。
安装保留此前包，使用原有 `CODEX_HOME` 和 `codex` 命令；替换日常 CLI 前先在隔离目录验证完整包。
回退时选择以前的包或重装官方 CLI，保留用户配置、会话和凭据。不得在输出中暴露凭据。

平台及路径的具体接口由 README、发布工作流和 `scripts/fork/` 的代码定义；先读源码再操作。
每次交付明确区分本地验证、GitHub CI、Release 产物及本机实际运行版本。
