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

## 上游同步与防回退

同步新 alpha 时允许内部版本跟随上游变化，并重新建立缓存；不得为了命中旧缓存
保留错误的编译基线。新增展示或身份入口时先判断它需要分发版本还是内部协议版本，
需要分发身份的调用沿用集中 getter。保留同一 binary、不同清单的原生回归和
app-server 验收，避免仅看到 Cargo 构建成功就误报已安装版本正确。
