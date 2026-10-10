---
name: codex-fork-maintenance
description: 维护 happy-proto/codex 个人 fork；用于判断修改所属功能层、维护 GitHub PR stack、同步上游 alpha、验证 fork 集成版本、发布或安装 fork CLI。
---

# Codex fork 维护

先读仓库通用约定 [AGENTS.md](../../../AGENTS.md)，再按当前任务读取操作参考。

进行 fork 改动时，默认按 [upstream.md](references/upstream.md) 同步最新已发布的上游 alpha，
将本轮改动和同步结果一起验证、统一推送，以复用完整 CI；适用边界和例外见该参考。

## 按需读取

- 定位修改层、交付代码及调整 stack：读 [stack.md](references/stack.md)。
- 同步 alpha 或分析上游变化：读 [upstream.md](references/upstream.md)。
- 构建、发布、安装、更新和回退：读 [release.md](references/release.md)。
