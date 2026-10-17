# 发布版本与编译缓存分离

## 动机

同一上游 alpha 的 `.fork.N` 只表示分发修订。若每次打包都重写 Cargo workspace
和 lockfile 中的内部版本，会使没有源码变化的大量 crate 失去缓存，增加构建和测试耗时。
发布身份需要准确，同时应保留未变化编译输入的复用能力。

## 行为和约束

- 内部 Cargo workspace 和 lockfile 保持选定上游 alpha 的版本；发布脚本只把
  `.fork.N` 写入 `codex-package.json` 等分发清单，不用 `cargo set-version`
  为每次分发重写依赖图。
- CLI、TUI、exec 展示和 daemon 身份通过 `BuildInfo::package_version()`
  读取当前包清单。未打包的可执行文件回退到 `CARGO_PKG_VERSION`。
  内部协议与遥测中保留的 Cargo 版本继续表示编译基线，不能不加区分地替换所有版本字段。
- Git commit 通过 `codex_build_info::initialize!` 在最终可执行文件处注入，
  避免把每个 Git 提交变成共享 Rust 库的编译输入；发布清单仍记录准确源码 commit。
- Cargo 与 sccache 继续校验源码、依赖、工具链、目标和编译参数。
  固定内部版本只消除分发修订造成的失效，不绕过真实输入变化的重编译。
- 构建和测试共用 sccache 后端命名空间，由编译缓存键区分输入；关闭 incremental。
  Cargo 下载缓存独立维护，不缓存整个 target、bin 或每个 job 的编译对象闭包。
  两类缓存只允许默认分支写入，其它分支的手动验证只读。

## 入口和验证

- [发布脚本](../../scripts/fork/release.py)：选择统一分发版本、调用上游 package builder、
  写包清单并执行产物验收。
- [版本解析](../../codex-rs/build-info/src/lib.rs) 和
  [Rust 回归](../../codex-rs/build-info/src/build_info_tests.rs)：包版本回退和构建身份。
- [原生包版本回归](../../scripts/fork/test_package_version.py)：同一个未修改的 native
  binary 放入两个不同版本的清单，`--version` 必须分别匹配；两个平台都在 CI 执行。
- [自有 CI](../../.github/workflows/fork.yml)：保留 sccache JSON 统计与 Cargo timing
  artifact。完整包验收同时检查 CLI 版本、code-mode host、rg 和 app-server
  `initialize` 的 `userAgent`，并核对发布清单的源码与平台包摘要。

评估缓存收益时区分首次预热与热缓存，比较相同源码、工具链、runner、依赖与编译参数；
单独改变 `.fork.N` 后，大 crate 应能复用。分别看 crate 编译、链接、队列、下载和
缓存读写耗时，不能把整段 job 时长或不可缓存请求都归为缓存 miss。
runner 镜像升级与缓存改造分别验收，不把不同系统的运行当作同环境对照。

## 缓存诊断

[诊断脚本](../../scripts/fork/cache_diagnostics.py) 从 sccache daemon 日志提取
`diagnostics.json`，与原有统计一起保存在 `fork-*-sccache-*` artifact：

- 记录源码 commit、Rust 源码树、内部和分发版本、工具链及 runner 镜像，核对对照条件。
- 记录缓存键、命中结果、输入哈希和读取耗时、实际编译耗时及成功写入时间。
- 为 ts-rs、absolute-path、V8、code-mode、config、protocol、core 和 TUI 保存直接依赖产物的文件名与摘要，
  区分源码或编译参数变化与依赖产物变化。
- 写入失败只保留存储错误类别、HTTP 状态和已知缓存键，不上传可能含签名 URL 的原始日志。
- 另外采集后端读取警告。sccache 0.18.0 的远端存储会把部分读取异常转成普通 miss，
  只在日志中警告，因此原有 `cache_read_errors` 为零仍不能排除读取异常；
  用 `backend_read_errors` 和 `backend_read_error_kinds` 核对这些被降级的错误。
- `storage_read_failures` 保留 OpenDAL 的读取失败类别，包括普通的缓存条目不存在；
  用 `lookup_non_ok` 和 `blob_not_found` 区分缓存服务未返回条目与底层对象缺失。
  它和后端读取警告属于不同层的观察，同一次失败可能同时出现，不把计数相加。

同源码重跑时先比对大 crate 的缓存键和依赖摘要；键不同应先查输入变化，键相同但条目
不可用时再查写入失败和淘汰，不能仅凭仓库接近容量上限认定根因。
诊断日志格式随 sccache 版本维护，[脚本回归](../../scripts/fork/test_cache_diagnostics.py)
覆盖命中与编译结果、依赖摘要变化和原始错误信息不外泄。

### 构建中的实时快照

[构建观察器](../../scripts/fork/build_observer.py) 包裹原有发布构建命令，每分钟向
Actions 日志和 `fork-sccache/progress.ndjson` 输出一次快照，不改变 Cargo 参数：

- runner 的 CPU 数、load average、空闲磁盘，编译与链接进程的 CPU、RSS、活跃 crate。
- macOS 的内存页计数、页大小、压缩页和 swap 使用量；Linux 的可用内存与 swap。
- sccache 命中、未命中和读写错误计数；后端降级的读取错误仍以日志诊断为准。
- 增量读取 daemon 日志，实时统计后端读取、存储读取和写入错误的类别与 HTTP 状态，
  包括未计入公开读取错误计数的 429。三个层级分别统计，不相加；不输出原始日志。
- 构建退出码与经过时间；观测或 artifact 写入失败不能改变实际构建结果。

进程 CPU 百分比来自 `ps`，是进程生命周期的平均值，不冒充采样窗口内的瞬时利用率。
统计范围为整个 runner，包含 sccache daemon 启动的编译器；在共享 WSL 上运行时应避开
其它构建。只输出预定字段，不输出原始命令行、环境变量、缓存地址或未筛选的异常。
取消时把信号转发给构建进程组，保留原有 `always()` 诊断与 artifact 收集步骤。

### 用小型依赖复现代替完整构建

发现相同缓存键对应的重编译产物摘要变化后，先隔离该依赖，避免重复构建 core、TUI
和最终 binary 才能判断一次假设。[小型复现脚本](../../scripts/fork/probe_ts_rs_cache.py)
对应 `ts-rs 11.1.0` 及 [上游宏输出确定性修复 #488](https://github.com/Aleph-Alpha/ts-rs/pull/488)：

- 手动运行 `fork.yml` 并设置 `macos_cache_probe=true`，仅在 `macos-latest` 运行小型
  fixture，跳过完整构建、其它 Rust 测试和发布；Cargo 下载缓存只读。
- 原版和隔离副本中的上游修复各编译三次，只清理 `ts-rs` 产物并保留依赖。
  禁用编译缓存，检查每组依赖字节一致，防止把恢复同一缓存对象误判为可复现构建。
- 从 workspace lockfile 提供传递依赖版本，记录锁文件摘要，并拒绝 fixture 解析出
  该锁文件中没有的版本，避免只固定直接依赖而让小型复现跟随 registry 更新。
- release codegen units、strip 等设置跟随 workspace profile，debug、LTO 和 split
  debuginfo 对齐发布 CI 的覆盖值，记录实际配置；fixture 不模拟全仓 feature 合并。
- 保存逐次 `.rmeta`、摘要、命令日志和汇总。原版需实际出现差异，修复版需保持一致，
  否则本次验证失败或证据不足；不修改 workspace 或全局 registry 中的依赖源码。
- 这仅验证选定版本及 fixture。工作区升级该依赖后重新核对小型复现的适用范围，
  不把有限次数的样本当成所有 crate 与所有宏输入都可复现的证明。

WSL 可作为额外的快速验证环境：对齐 CI 的 Rust、Python、锁文件、features、profile、
并行度和 incremental 设置，使用独立 checkout/fixture 与 target，不改动已有工作区。
该脚本用 `--target x86_64-unknown-linux-gnu` 验证 Linux；每个平台内部比较重复编译，
不直接比较跨平台 `.rmeta` 摘要或总耗时。需要观察 sccache 行为时使用 CI 固定版本和
独立本地缓存，不依赖 GitHub Actions 的运行时凭据。真实 GHA 后端读取、429 和淘汰
仍在 Actions 验证；macOS 链接、签名与内存压力留给小型 Mac job 或最终集成构建。

固定的 sccache 0.18.0 未包含 [上游的 `.rmeta` 流式通知修复](https://github.com/mozilla/sccache/pull/2875)。
缓存未命中时，延后的通知会削弱 Cargo 流水线，也会让 timing 图中的前端和 codegen
分段失真；用单元总耗时和缓存诊断判断复用，不把图中的长前端段直接当成实际前端耗时。
升级时检查新稳定版是否包含该修复，并重新做冷、热缓存对照；常规发布继续使用稳定版。

## 上游同步与防回退

同步新 alpha 时允许内部版本跟随上游变化，并重新建立缓存；不得为了命中旧缓存
保留错误的编译基线。新增展示或身份入口时先判断它需要分发版本还是内部协议版本，
需要分发身份的调用沿用集中 getter。保留同一 binary、不同清单的原生回归和
app-server 验收，避免仅看到 Cargo 构建成功就误报已安装版本正确。
