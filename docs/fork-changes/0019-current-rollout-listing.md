# 会话列表只展示当前 rollout

## 动机与行为

`thread/revert` 保留逻辑 thread ID，创建新的 immutable rollout，并更新 SQLite 中的
当前路径。旧文件仍可能是 `history_base` 引用的历史前缀，必须保留。

上游的文件扫描列表按物理文件生成记录，来源或 provider 等过滤条件会使 `thread/list`
返回扫描结果。多份 rollout 因而可能让同一个 thread ID 在同页或跨页重复出现。
相关上游报告为 [openai/codex#43017](https://github.com/openai/codex/issues/43017)。

有 SQLite 当前记录时，列表只接受该记录选中的 rollout，不按文件创建时间或 mtime
猜测当前版本。过滤发生在分页计数之前，扫描继续补齐页面；历史文件不删除、不改写。
压缩前后的 `.jsonl` / `.jsonl.zst` 采用相同的逻辑路径比较。

## 实现边界

- [文件扫描](../../codex-rs/rollout/src/list.rs) 增加 crate 内部的数据库感知入口，
  在生成 `ThreadItem` 时排除 SQLite 已明确替换的路径。
- [列表调度](../../codex-rs/rollout/src/recorder.rs) 为活动和归档列表、搜索以及正反排序
  传入同一个状态数据库上下文；公开 Rust 接口与 app-server 协议保持不变。
- 数据库不可用、查询失败或缺少该 thread 的记录时，保留原有文件回退行为。
  没有权威当前指针时不凭时间替用户选择历史分支，因此这类情况下仍可能出现重复。
- 本地 `resume` 和 `agents` 原本优先使用数据库列表，本改造主要修复文件扫描路径。
  同时间戳游标的独立分页问题、缺少预览导致的隐藏问题，以及 Doctor 对合法历史的误报
  均不属于本改造。

## 回归与维护

[回归测试](../../codex-rs/rollout/src/recorder_tests.rs) 构造一个 SQLite 选中的 rollout、
多份创建时间和 mtime 更晚的同 ID 历史，以及未入库的普通会话，检查：

- 活动和归档、压缩和未压缩、创建时间和更新时间、正反排序的分页不重复、不漏普通会话。
- 当前 rollout 不满足来源或 provider 过滤时，旧 rollout 不能作为匹配结果泄漏。
- SQLite 当前路径保持不变，历史文件仍存在且可解析。

Fork CI 运行 `codex-rollout --lib`，同时覆盖既有数据库缺失、关闭与读修复回退测试。
上游同步时重点检查文件扫描入口、当前路径选择规则、压缩路径解析和扫描上限。
只有上游在分页计数前采用相同的权威路径规则后，才移除这项改造。
