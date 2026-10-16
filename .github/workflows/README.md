# Fork CI 与发布

`fork.yml` 在 Ubuntu 上执行脚本检查和 Rust 回归测试，并行构建 Linux AMD64、macOS ARM 完整包。
Linux 使用原生 Cargo，macOS 使用 cargo-zigbuild、Zig 和固定摘要的 SDK 交叉编译，
由 rcodesign 添加 ad-hoc hardened runtime 签名和上游 entitlements。工具链升级需验证
ARM64 架构、动态依赖、签名和目标系统上的实际运行。

推送 `main` 后，两个平台构建及 Rust 测试全部成功才发布。发布脚本校验源码仍为默认分支头，
聚合平台摘要，上传后下载复核，再公开不可变 Release，保留最新十个已发布 fork 版本。
平台包名、版本号与安装更新契约见 [分发说明](../../docs/fork-changes/0001-distribution-and-updates.md)。

完整平台包只在默认分支构建，包括手动触发。其它分支手动运行默认仅执行脚本检查，
即使指定 `publish=true` 也跳过完整包构建和发布；默认分支手动发布需指定 `publish=true`。
`tests_only` 显式运行完整 Rust 回归并跳过打包，`tui_test_filter`、`core_test_filter` 运行对应定向测试。
只有默认分支可以写入下载和编译缓存，其它分支只读。

Linux 包在 CI 执行 CLI、资源、app-server、终端目录与包版本验收。
macOS 包在 Linux 检查架构和重复动态库加载命令，签名后再计算归档摘要；
在 Mac 解包后运行 `scripts/fork/release.py smoke --package <目录>`，
工具链更新时还需验证 V8 工具调用和终端启动、恢复；Linux 静态校验不能替代真实运行。

构建和测试的 Cargo timings、资源采样与缓存统计分别保存在对应 artifact 和 job summary。
