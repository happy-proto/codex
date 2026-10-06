# 发布、更新和安装

`fork` 的每次更新在验证通过后自动发布，首期仅 `aarch64-apple-darwin`。
版本沿用选定上游 alpha 加 `.fork`，无 fork 序号。同一 alpha 下重发更新同名 Release。
软件版本不能唯一标识构建：用源 SHA、包摘要和发布清单识别实际构建。

构建复用上游完整 package builder，包含 CLI、code-mode host、rg 和 zsh 等必要资源。
macOS 使用 ad-hoc 签名并保留上游 entitlements；不配置官方 Azure、R2、npm、WinGet 或网站发布设施。
下载 CI/Release 产物做隔离安装与轻量运行验收。
标准 macOS runner 使用 release 优化、关闭 Thin LTO；首期验证中仅最终链接优化就耗时约 29 分钟。
测试与产物构建并行；测试保留 release 条件编译及关闭 debug assertions，禁用测试优化以控制编译成本。

修改发布流程时检查：版本与选定 alpha 一致、源码属于完整 fork 分支、签名后的资源摘要正确、
产物先上传验证再更新清单、同版本更新可被识别、下载损坏不会切换 current、并发发布不会让旧构建覆盖新构建。
不要仅测 `codex --version`：还要验证 package 资源、code-mode host、app-server 启动及实际更新行为。

更新只提示，由用户执行 `codex update`。不自动更新到官方源，不启用后台自动升级。
安装保留此前包，使用原有 `CODEX_HOME` 和 `codex` 命令；替换日常 CLI 前先在隔离目录验证完整包。
回退时选择以前的包或重装官方 CLI，保留用户配置、会话和凭据。不得在输出中暴露凭据。

平台及路径的具体接口由 README、发布工作流和 `scripts/fork/` 的代码定义；先读源码再操作。
每次交付明确区分本地验证、GitHub CI、Release 产物及本机实际运行版本。
