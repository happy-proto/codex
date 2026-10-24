---
name: codex-fork-maintenance
description: 维护 happy-proto/codex 个人 fork；用于更新编号改造记录、整理或重写 fork 提交历史、验证 fork 版本、发布或安装 fork CLI。同步上游 alpha 或分析上游变化使用 codex-upstream-sync。
---

# Codex fork 维护

先读仓库通用约定 [AGENTS.md](../../../AGENTS.md)，再按当前任务读取操作参考。

整理提交历史按 [history.md](references/history.md) 执行，保留已选基线；
其它 fork 改动默认使用 [codex-upstream-sync](../codex-upstream-sync/SKILL.md) 同步最新已发布的上游 alpha，
并检查内置第三方 skills 更新，将本轮改动和同步结果一起验证、统一推送，以复用完整 CI。
适用边界和例外见该 skill。

## 自有脚本

为本 skill 新增或修改可复用 Python CLI 时，使用 `$dcjanus:uv-cli-creator`，
入口放在本 skill 的 `scripts/` 目录；按该 skill 初始化和维护 PEP 723 依赖、可执行入口、
参数帮助及验证，不按 `$dcjanus:python-execution` 的临时脚本方式编写。
环境支持时直接执行脚本路径，其它环境使用 `uv run --script`；通过参数显式传入目标仓库路径。
仓库与 CI 共用的 `scripts/fork/` Python CLI 同样通过可执行路径或 `uv run --script` 调用；
运行时要求见仓库约定，测试使用 `scripts/fork/run_tests.py`，可按 `--help` 选择测试模块或文件。

## 按需读取

- 定位改造点、维护编号记录和交付代码：读 [changes.md](references/changes.md)。
- 同步 alpha 或分析上游变化：使用独立 skill [codex-upstream-sync](../codex-upstream-sync/SKILL.md)。
- 按改造点归并、拆分或重写 fork 提交历史：读 [history.md](references/history.md)。
- 构建、发布、安装、更新和回退：读 [release.md](references/release.md)。
