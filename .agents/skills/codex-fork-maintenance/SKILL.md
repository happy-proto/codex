---
name: codex-fork-maintenance
description: 维护 happy-proto/codex 个人 fork。用于同步或分析上游 alpha、维护 fork 改造和历史、验证并发布版本、维护中文 Release notes，或安装 fork CLI。
---

# Codex fork 维护

先读仓库通用约定 [AGENTS.md](../../../AGENTS.md)，再按当前任务读取操作参考。

维护 fork 默认同步最新已发布的上游 alpha，用户指定版本或要求保留基线时遵循其范围。
只读分析、单独修改本 skill 或补写已有 Release notes 时保留当前基线。
用户要求同步上游时，默认包含提交、推送及跟进到实际发布并验证；
本地或发布范围限制优先，安装单独授权。

**每次同步都验证内置 `codex-session` 与 `codex task` 至少覆盖候选上游 TUI MCP 的全部能力。**
默认关闭 TUI MCP 时仍须达到等价或超集。缺口的处理及交付门槛见
[同步与发布](references/upstream-sync.md#内置会话管理-skill-的发布门槛)。

## 自有脚本

为本 skill 新增或修改可复用 Python CLI 时，使用 `$dcjanus:uv-cli-creator`，
入口放在本 skill 的 `scripts/` 目录；按该 skill 初始化和维护 PEP 723 依赖、可执行入口、
参数帮助及验证，不按 `$dcjanus:python-execution` 的临时脚本方式编写。
环境支持时直接执行脚本路径，其它环境使用 `uv run --script`；通过参数显式传入目标仓库路径。
仓库与 CI 共用的 `scripts/fork/` Python CLI 同样通过可执行路径或 `uv run --script` 调用；
运行时要求见仓库约定，测试使用 `scripts/fork/run_tests.py`，可按 `--help` 选择测试模块或文件。

## 按需读取

- 定位改造点、维护编号记录和交付代码：读 [changes.md](references/changes.md)。
- 同步 alpha、核对内置 skills、跟进发布及维护中文 Release notes：读 [同步与发布](references/upstream-sync.md)。
- 按改造点归并、拆分或重写 fork 提交历史：读 [history.md](references/history.md)。
- 构建、发布、安装、更新和回退：读 [release.md](references/release.md)。
