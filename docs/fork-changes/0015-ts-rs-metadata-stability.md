# ts-rs 宏输出与编译元数据稳定性

## 动机

`ts-rs 11.1.0` 的 derive 宏使用随机迭代顺序的集合生成依赖访问和泛型约束。
相同源码、工具链、编译参数和依赖字节下，重新编译也可能得到不同的 `.rmeta`。
当远端缓存读取失败导致一次重编译时，变化会沿依赖图传播，使 core、TUI 等大 crate
原有缓存键失效。固定内部版本不能解决这种失效。

## 实现和边界

- 回移 [ts-rs #488](https://github.com/Aleph-Alpha/ts-rs/pull/488) 的确定性修复，
  保留 11.1.0 API，不升级到新的主版本。
- [宏源码副本](../../codex-rs/vendor/ts-rs-macros/README.fork.md) 保留许可证和来源，
  两处参与输出的 `HashSet` 改为保持插入顺序的 `IndexSet`。
- 删除 crate 顶层无效的 `#![macro_use]`，保留 `utils` 模块上的宏导入和
  `#![deny(unused)]`，使副本兼容 Rust 1.99.1 的属性检查。
- [workspace manifest](../../codex-rs/Cargo.toml) 使用 Cargo source override；
  宏副本不是 workspace 成员，不参与 Codex 内部版本调整。
- `indexmap 2` 使用锁文件中已有版本，不升级其它依赖。
- 修复消除这一条随机元数据传播路径；不会修复 GitHub Cache 的 429，也不会使
  源码实际变化、平台不同、编译参数不同的产物共用缓存。

## 验证和观测

[小型复现](../../scripts/fork/probe_ts_rs_cache.py) 在相同依赖字节下强制重编译
`ts-rs` 三次，对照原版与修复版的 `.rmeta` 摘要。macOS 原版三个摘要不同，修复版
三个摘要相同；Linux/WSL 也复现同样差别。有限次数样本只证明该路径和所选输入。

WSL 上的实际 workspace 对照另用 `cargo check --release -p codex-protocol`，
只清理 `ts-rs` 后重复三次检查，保持其它依赖产物。原版的 `ts-rs`、absolute-path 和
protocol 各自产生三个不同摘要，修复后三层各自保持一个摘要，验证变化沿真实依赖图
传播。小型下游的 sccache 0.18.0 对照中，强制重编译上游后，原版连续三次 miss，
修复版连续三次 hit；这些验证不替代平台发布构建的耗时对照。

`--patched-macros codex-rs/vendor/ts-rs-macros` 可直接验证实际维护的副本，
并保存源码摘要，防止只测了等价的临时补丁却遗漏正式依赖。默认禁用 sccache，
避免把恢复的同一对象误当成确定性证据。完整发布构建仍负责验证真实依赖图和包行为。

## 上游同步

同步时检查上游所选 `ts-rs` 版本是否已经包含修复。兼容版本已包含时，删除 source
override、副本及其 workspace exclude，保留此记录并标明吸收版本；先验证依赖稳定性，
再根据实际缓存键和编译耗时判断收益，不直接比较不同 runner 的整段时长。
