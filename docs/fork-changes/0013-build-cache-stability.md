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
- CLI 参数解析、命令分发及其单元测试放在 `codex_cli` 库中；最终 `main.rs` 只保留
  原有 allocator、构建身份初始化和库入口调用。这样只有提交身份变化时，CLI 的主体
  也能参与 sccache 复用；不降低正式产物的 opt-level 或 codegen units。
  原有 sandbox 参数类型和函数继续由库根导出，命令行参数、退出状态及初始化顺序不变。
- Cargo 与 sccache 继续校验源码、依赖、工具链、目标和编译参数。
  固定内部版本只消除分发修订造成的失效，不绕过真实输入变化的重编译。
- 构建和测试共用 sccache 后端命名空间，由编译缓存键区分输入；关闭 incremental。
  Cargo 下载缓存独立维护，不缓存整个 target、bin 或每个 job 的编译对象闭包。
  两类缓存只允许默认分支写入，其它分支的手动验证只读。
- Linux 发布构建与测试共用 Cargo 下载缓存，由默认分支的发布构建保存。
  恢复步骤单独排除测试专用的编译参数，避免为同一批源码保存重复 archive；
  它不改变后续 Cargo 命令的环境，也不影响 sccache 对真实编译参数的校验。
- `ci-test` 保留 test profile 的 debug assertions，关闭调试符号并使用 256 个 codegen
  units；这些设置由 workflow 全局环境传给测试。release 测试继续覆盖 release-only
  代码，保持关闭 debug assertions，以未优化的 harness 缩短验证耗时。
- 完整测试按 `ci-test` 和 release profile 分成两个并行 job，分别复用各自的编译结果，
  保留原有测试参数与覆盖范围。定向 core 或 TUI 验证只启动对应组；所有测试组通过后
  才允许发布。两组使用独立的 timing、缓存诊断与失败快照 artifact 名称。
- 手动运行时可设置 `tests_only=true`，只验证测试分组，跳过平台打包并禁止发布。
  它与已有的定向 filter 组合使用；默认分支的 push 仍执行完整构建和发布门禁。

## 入口和验证

- [发布脚本](../../scripts/fork/release.py)：选择统一分发版本、调用上游 package builder、
  写包清单并执行产物验收。
- [版本解析](../../codex-rs/build-info/src/lib.rs) 和
  [Rust 回归](../../codex-rs/build-info/src/build_info_tests.rs)：包版本回退和构建身份。
- [CLI 入口](../../codex-rs/cli/src/main.rs) 与 [库实现](../../codex-rs/cli/src/lib.rs)：
  CI 运行完整 CLI 库单元测试及原有 features、MCP 集成测试，覆盖参数解析与输出快照。
  评估入口拆分时，只改变 `STABLE_GIT_COMMIT`，检查其它 crate 未重新编译，保持相同
  正式优化参数，并比较实际可执行文件的输出与启动耗时；首次库编译单独计入预热。
- [原生包版本回归](../../scripts/fork/test_package_version.py)：同一个未修改的 native
  binary 放入两个不同版本的清单，`--version` 必须分别匹配；两个平台都在 CI 执行。
- [自有 CI](../../.github/workflows/fork.yml)：保留 sccache JSON 统计与 Cargo timing
  artifact。完整包验收同时检查 CLI 版本、code-mode host、rg 和 app-server
  `initialize` 的 `userAgent`，并核对发布清单的源码与平台包摘要。

评估缓存收益时区分首次预热与热缓存，比较相同源码、工具链、runner、依赖与编译参数；
单独改变 `.fork.N` 后，大 crate 应能复用。分别看 crate 编译、链接、队列、下载和
缓存读写耗时，不能把整段 job 时长或不可缓存请求都归为缓存 miss。
runner 镜像升级与缓存改造分别验收，不把不同系统的运行当作同环境对照。

### 平台差异的测量边界

比较 macOS 与 Linux 前，记录实际 CPU、内存、架构、runner 镜像和工具版本。
GitHub 的标准 ARM Mac 与 x64 Linux runner 并非等配置；大 crate 冷编译时，
macOS 的压缩内存与 swap 可以放大差距，热缓存无 swap 时不能沿用同一解释。
内存压力表示有关联，不能据此声称已经分离了 CPU、操作系统和目标架构的因果贡献。

把 Cargo、strip、签名、原生 smoke、归档和产物验收分段计时。
使用校验过摘要的已发布包单独验证后几项，可避免每次为确认打包或测试夹具问题而
重编整个 Rust 工作区。跨平台的包字节不同，各平台内先做相同输入的对照。
终端验收的模型服务只绑定回环字面地址，不应依赖反向 DNS；约束见
[OSC 7 记录](0002-terminal-working-directory.md)。

同一缓存 key 上轮命中、本轮读取遇到 429 并退回编译、再次写入又报告已存在，
说明读取失败不能当成源码变化或缓存被淘汰。核对该 crate 是否处于关键路径，
不能直接把它的编译秒数加到 job 差值中。sccache 的 GHA 重试问题另见
[上游 issue #2821](https://github.com/mozilla/sccache/issues/2821)；升级时核对固定版本的
实际实现并重新验证，不能把上游建议误写为本 fork 已启用的重试修复。

### 防回退依据

| 优化或约束 | 自动化覆盖与重测方式 |
| --- | --- |
| 分发版本不改变共享库编译身份 | [发布脚本回归](../../scripts/fork/test_release.py) 校验上游基线、统一版本；[包版本回归](../../scripts/fork/test_package_version.py) 用同一真实 binary 验证不同清单；[Rust 身份回归](../../codex-rs/build-info/src/build_info_tests.rs) 覆盖身份初始化。 |
| CLI 主体留在可缓存的库，保持正式优化参数 | CI 继续运行 CLI 库的完整测试与快照；入口或编译链调整时，固定其它条件，只改 `STABLE_GIT_COMMIT` 重建，核对库缓存键、实际 rustc 选项和启动行为。该性能对照不能由参数解析测试代替。 |
| 两个测试 profile 并行且保持原覆盖范围 | workflow 分别执行完整 `ci-test` 与 release 测试，全部成功后发布；[观察器回归](../../scripts/fork/test_build_observer.py) 和[发布脚本回归](../../scripts/fork/test_release.py) 覆盖 Cargo 参数、环境、`--timings` 与失败退出状态的透传。 |
| 缓存、资源和 timing 报告可信 | [缓存诊断](../../scripts/fork/test_cache_diagnostics.py)、[观察器](../../scripts/fork/test_build_observer.py)、[汇总回归](../../scripts/fork/test_build_report.py) 覆盖缓存键、后端限流、敏感原始错误不外泄、多 invocation 去重和 macOS 物理压缩页。 |
| 原生验收不等待外部 DNS | [终端验收回归](../../scripts/fork/test_terminal_cwd.py) 在禁止反向 DNS 时验证回环 HTTP 服务；平台构建继续验证真实 CLI 启动与 resume。 |
| 压缩调整不破坏交付内容 | [发布脚本回归](../../scripts/fork/test_release.py) 覆盖 xz 无损内容与可执行权限；平台 CI 继续验收签名后原生包及安装契约；压缩性能另按相同 tar 输入重测。 |

耗时报告用于发现回退，不使用固定秒数或跨平台倍率作为单次 CI 成败门槛。
修改上述入口时，除功能回归外，保留受影响环节的冷、热缓存对照和实际编译参数，
明确自动化已证明的行为与仍需受控性能实验验证的部分。

## 缓存诊断

[诊断脚本](../../scripts/fork/cache_diagnostics.py) 从 sccache daemon 日志提取
`diagnostics.json`，与原有统计一起保存在 `fork-*-sccache-*` artifact：

- 记录源码 commit、Rust 源码树、内部和分发版本、工具链及 runner 镜像，核对对照条件。
- 记录缓存键、命中结果、输入哈希和读取耗时、实际编译耗时及成功写入时间。
- 为选定的 crate 记录显式 `-C` 编译选项，并保存 release 与 ci-test 的 profile 环境；
  缺省选项不推断为关闭，用于核对实际编译条件，避免只看 workflow 声明。
- 为 ts-rs、absolute-path、V8、code-mode、config、protocol、core、TUI 和 CLI 库保存直接依赖产物的文件名与摘要，
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
固定版本的 [sccache Rust 缓存键](https://github.com/mozilla/sccache/blob/v0.18.0/src/compiler/rust.rs#L1486-L1523)
还包含多数 `CARGO_` 环境变量；增加其它 profile 的变量也可能让当前 profile 的缓存
失效。对照实验保持这些变量一致，将配置变更造成的首次预热与后续热缓存收益分开。
诊断日志格式随 sccache 版本维护，[脚本回归](../../scripts/fork/test_cache_diagnostics.py)
覆盖命中与编译结果、依赖摘要变化和原始错误信息不外泄。

### 构建中的实时快照

[构建观察器](../../scripts/fork/build_observer.py) 包裹原有发布构建命令；
[测试入口](../../scripts/fork/cargo-observed.sh) 为每条 Cargo 测试命令复用同一观察器，
不改变参数、工作目录、环境或退出状态。每分钟记录一次资源快照，终端显示摘要，
完整明细写入 `fork-sccache/progress.ndjson`：

- runner 的 CPU 数、load average、空闲磁盘，编译与链接进程的 CPU、RSS、活跃 crate。
- macOS 的内存页计数、页大小、压缩页和 swap 使用量；Linux 的可用内存与 swap。
- sccache 命中、未命中和读写错误计数；后端降级的读取错误仍以日志诊断为准。
- 增量读取 daemon 日志，实时统计后端读取、存储读取和写入错误的类别与 HTTP 状态，
  包括未计入公开读取错误计数的 429。三个层级分别统计，不相加；不输出原始日志。
- 构建退出码与经过时间；观测或 artifact 写入失败不能改变实际构建结果。
- 每次命令使用独立 invocation ID，汇总时分别取其最终耗时，不能把采样经过时间相加。

[构建汇总](../../scripts/fork/build_report.py) 生成同一 artifact 下的 `report.json`
及 Actions job summary，显示缓存命中、不可缓存请求、后端读取错误、资源峰值和最慢
Cargo 单元。读取全部带时间戳的 timing 报告，排除指向最后一次命令的别名，避免最后
一条命令完全命中时把整轮测试误报成零秒。单元累计时长允许并行，不视为 job 总耗时；
资源峰值来自周期采样，不能作为进程真实峰值的精确测量。

进程 CPU 百分比来自 `ps`，是进程生命周期的平均值，不冒充采样窗口内的瞬时利用率。
统计范围为整个 runner，包含 sccache daemon 启动的编译器；在共享 WSL 上运行时应避开
其它构建。只输出预定字段，不输出原始命令行、环境变量、缓存地址或未筛选的异常。
取消时把信号转发给构建进程组，保留原有 `always()` 诊断与 artifact 收集步骤。

### 依赖元数据的确定性

相同缓存键对应的重编译产物摘要变化时，先隔离该依赖；保持工具链、依赖字节、
features 和 profile 一致，禁用编译缓存，避免将恢复的同一对象误判为可复现构建。
宏输出顺序造成的元数据变化及维护边界见 [ts-rs 确定性记录](0015-ts-rs-metadata-stability.md)。
不同平台分别检查重复编译，不直接比较跨平台的元数据摘要。

固定的 sccache 0.18.0 未包含 [上游的 `.rmeta` 流式通知修复](https://github.com/mozilla/sccache/pull/2875)。
缓存未命中时，延后的通知会削弱 Cargo 流水线，也会让 timing 图中的前端和 codegen
分段失真；用单元总耗时和缓存诊断判断复用，不把图中的长前端段直接当成实际前端耗时。
升级时检查新稳定版是否包含该修复，并重新做冷、热缓存对照；常规发布继续使用稳定版。

## 上游同步与防回退

同步新 alpha 时允许内部版本跟随上游变化，并重新建立缓存；不得为了命中旧缓存
保留错误的编译基线。新增展示或身份入口时先判断它需要分发版本还是内部协议版本，
需要分发身份的调用沿用集中 getter。保留同一 binary、不同清单的原生回归和
app-server 验收，避免仅看到 Cargo 构建成功就误报已安装版本正确。
同步 CLI 时把新增命令和私有模块继续放在库入口，保留 binary 的构建身份与 allocator
初始化；单元快照使用库 crate 名 `codex_cli`，不能因入口变薄而漏跑原有 CLI 测试。
执行服务的命令及认证实现由上游独立 `codex-exec-server-cli` crate 提供，CLI 库引用其公开入口；
不保留已迁出的私有模块，执行服务遥测仍通过集中 getter 读取包版本。
