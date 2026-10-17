# ts-rs-macros 确定性修复

来源为 crates.io 的 `ts-rs-macros 11.1.0`，原始 crate SHA-256：
`ee6ff59666c9cbaec3533964505d39154dc4e0a56151fdea30a09ed0301f62e2`。
保留原始源码、版权和 MIT 许可证。

回移 [上游 #488](https://github.com/Aleph-Alpha/ts-rs/pull/488)
（`a6bbbd183aae4142c273e3ff2042d30a3b1a808f`）的宏输出顺序修复：
`src/deps.rs` 和 `src/lib.rs` 中参与输出的集合改用 `IndexSet`，增加 `indexmap 2`
依赖。crate 版本和公共 API 保持 11.1.0。

移除 crate 顶层无效的 `#![macro_use]`，避免 Rust 1.99.1 在
`#![deny(unused)]` 下拒绝编译；保留 `utils` 模块上的 `#[macro_use]` 和原有 lint。

工作区通过 `[patch.crates-io]` 使用此副本，锁文件以 `codex-rs/Cargo.lock` 为准。
本目录排除在 workspace 成员之外，避免套用 Codex 内部版本。升级到包含该修复的
兼容发布版后，移除 source override 和此副本，并重新执行元数据稳定性对照。

维护依据见 [编号记录 0015](../../../docs/fork-changes/0015-ts-rs-metadata-stability.md)。
