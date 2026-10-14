# 0004 功能列表输出格式

## 为什么改

功能列表既需要终端可读性，也需要脚本明确选择稳定的输出格式。

## 如何改

`codex features list` 支持 `--format plain|table|markdown|json`，默认表格。
输出按名称排序，使用加载配置后的有效开关状态。
JSON 使用 `features` 数组，每项包含 `name`、`stage` 和布尔值 `enabled`。

## 入口和验证

- [格式实现](../../codex-rs/cli/src/features_list.rs)、[命令入口](../../codex-rs/cli/src/main.rs)。
- [CLI 回归](../../codex-rs/cli/tests/features.rs)，包括格式和配置状态。

## 兼容和上游同步

**Breaking change**：默认文本从对齐列表改成表格；解析旧文本的脚本添加 `--format plain`。
同步时关注 feature catalog、状态字段和参数解析，保持结构化输出契约。
上游关联：[Issue #8397](https://github.com/openai/codex/issues/8397)；历史讨论：[PR #10](https://github.com/happy-proto/codex/pull/10)。
