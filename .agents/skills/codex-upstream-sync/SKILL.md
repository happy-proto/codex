---
name: codex-upstream-sync
description: 同步 happy-proto/codex fork 到最新已发布的上游 alpha，检查内置第三方 skills 更新，分析版本差异并维护中文 GitHub Release notes。
---

# Codex 上游同步

先读仓库通用约定 [AGENTS.md](../../../AGENTS.md)。

维护 fork 的改动默认包含同步最新已发布的上游 alpha，无需用户另行提出同步。
只读调查不改变基线；用户明确要求保留基线、暂不同步或指定版本时，遵循该要求。
单独补写已有 Release notes 或修改本 skill 时，不额外执行 alpha 同步或手动触发新版本构建；
已授权推送后的自动 CI 仍遵循仓库工作流。
用户要求同步上游时，默认包含提交、推送及跟进发布，直到对应版本实际发布并验证；
仅限本地、不推送、不发布或其它明确限制优先，安装仍需单独授权。
Actions 不自行 rebase 或强推分支。

本轮开始时确定已发布的候选；`scripts/fork/upstream-tag` 已对应该候选时，报告已是最新并跳过 alpha rebase，
仍须完成下面的内置 skill 更新检查。
将本轮改动与上游同步都准备、验证完毕后，先统一推送 `main`，避免不必要地重复触发完整 CI；
后续为修复问题所需的提交和推送不受这一约束。
本轮确定候选后不反复追逐新发行版。

## 自有脚本

为本 skill 新增或修改可复用 Python CLI 时，使用 `$dcjanus:uv-cli-creator`，
入口放在本 skill 的 `scripts/` 目录；按该 skill 初始化和维护 PEP 723 依赖、可执行入口、
参数帮助及验证，不按 `$dcjanus:python-execution` 的临时脚本方式编写。
环境支持时直接执行脚本路径，其它环境使用 `uv run --script`；通过参数显式传入目标仓库路径。
仓库与 CI 共用的 `scripts/fork/` Python CLI 同样通过可执行路径或 `uv run --script` 调用；
运行时要求见仓库约定，测试使用 `scripts/fork/run_tests.py`，可按 `--help` 选择测试模块或文件。

## 内置第三方 skills

每次同步上游时执行 `scripts/fork/sync_skills.py check --upstream`，
按 `scripts/fork/skills.lock.json` 中各来源的跟踪 ref 检查更新；检查失败不能视为没有更新。
有更新时，用 `scripts/fork/sync_skills.py update <skill> --ref <ref>` 更新对应快照和 lock，
保留原有跟踪 ref，避免改为固定 commit 后失去后续更新发现能力。
审查 skill 指令、引用文件和许可证差异，确认仍适用于 fork 的接入方式及运行程序；
执行离线 `check` 和 `test_sync_skills.py`，将快照及 lock 与 alpha 同步结果一起验证、交付。
来源及维护边界见 [0010 改造记录](../../../docs/fork-changes/0010-open-automation.md)。
最终回复简述哪些 skill 已更新、哪些没有更新；兼容性或检查失败时明确说明未完成项。

## Alpha 同步步骤

1. 确认工作树、授权和远端状态；只 fetch 所需的 origin 分支与 upstream 候选 tag，
   不拉取上游全部功能分支。记录旧 `main`、记录文件中的旧 tag、解析后的基线 SHA 和远端期望 SHA。
2. 从 `openai/codex` 的已发布、非 draft alpha Releases 中选择版本最高的候选，解析 annotated tag 到真实 commit。
   不用 `/releases/latest`（它不包含 alpha），不直接以 upstream/main 作为同步目标。
3. 校验旧 tag 是本地 `main` 的祖先。记录新旧 tag、SHA、祖先关系和高影响 diff。
   alpha tag 间可能不是快进；分叉时先查明上游发行历史，不把所有旧上游提交重放成 fork 修改。
4. 在临时 detached worktree 演练，以旧 tag 对应 SHA 明确划定自有提交范围，执行
   `git rebase --onto <new-upstream-sha> <old-upstream-sha>`，不凭当前 merge-base 推断旧基线。
5. 逐份核对编号改造记录：目的是否已被上游吸收、重构是否要求适配、生成物和对外契约是否仍一致。
   以源码和回归证据判断，不机械选 ours/theirs；更新 `scripts/fork/upstream-tag`，
   上游吸收后移除残留实现，保留记录并写明吸收版本；改变产品能力仍遵循用户授权。
6. 验证通过后，在授权范围内将本地 `main` 更新为演练结果。
   普通更新快进推送；rebase 后使用明确远端旧 SHA 的 force-with-lease，若远端已变更则重新核对，不能覆盖。
   回读默认分支和远端 SHA，清理临时 worktree。

## 跟进到发布

默认持续跟进最新提交的完整 CI 和发布，不将排队、运行中或仅推送成功视为完成。
CI 失败或同步过程中发现有依据的相关 bug 时，调查并修复，按风险验证后直接追加提交、推送，
再跟进新 SHA 的 CI 和发布，直到恢复；保留回归测试等项目要求。推送前核对远端状态，
若其他人更新了远端先整合其改动，不能覆盖。遇到无法自主解决的外部阻塞时，说明证据和剩余事项。

确认发布 job 成功后，回读非 draft Release、tag 对应源码 SHA、两个平台包和发布清单，
核对版本、资源及摘要与最新提交一致；具体发布契约见 [发布说明](../codex-fork-maintenance/references/release.md)。
随后完成下面的中文 Release notes，并回读确认；只有构建与安装信息的自动发布正文不算交付完成。
若本轮基线已是最新且没有新提交，核对已有对应版本的发布状态，不为制造新版本做空提交。
用户明确要求只检查、本地准备、不推送、不发布或提前停止时，按其范围执行并报告未完成的发布验证。

同步完成后的最终回复必须简要列举本次引入的主要上游变更，不能只报告已同步或 CI 状态。
以上一次记录的上游 tag 为基线，先写新旧 alpha 版本，再按功能与维护主题列出对用户有意义的变化；
说明具体新增能力、修复或行为变化，不用文件清单、提交标题堆砌或只有提交数量代替说明。
另行区分纯 rebase 和实际 fork 语义适配，说明各改造点的变化、验证、最新 SHA 的 CI/Release 状态、
待决事项和未执行的动作。摘要保持简短；只有高影响变化才展开细节。
自动发布成功不等于本机已经更新；安装是单独的验收步骤。

## 中文 Release notes

同步发布默认包含更新对应 GitHub Release 的中文说明；用户单独要求补写历史版本时，只更新指定范围。
通过 `github-cli` 操作，正文只保留版本差异、必要的兼容性与迁移说明及比较链接。
移除自动生成的固定构建与安装说明，不重复平台列表、提交元数据、版本编号、签名、更新命令及保留策略；
这些内容由仓库文档、Release 附件及发布清单承载。本版确有相关变化时仍说明其影响。
开头用一句话概括重点，正文以“上游更新”和“Fork 改进”两个 Markdown 标题区分来源，
各条用加粗的简短主题开头，一条说明一件事；不要跨来源合并成统一的改进、修复列表。
维护细节可放在所属分组的 `<details>` 中，重要行为与迁移说明直接展示，比较链接放末尾。
没有变化的来源用一句话说明，不为小版本或相同源码重发凑栏目。

- 按实际发布时间选取当前版本之前最近的已发布、非 draft fork Release，记录双方 tag、真实源码 SHA
  及各自源码中的 `scripts/fork/upstream-tag`。比较基准是上一个已发布 fork 版本，不能用失败构建、
  本轮开始时的本地 HEAD 或相邻上游 alpha 替代。前版已被保留策略清理时，尝试从发布日志恢复；
  无法恢复就明确缺失范围，不猜测。
- 分开说明上游变化与 fork 自有变化。上游基线变化时核对两个上游 tag 的提交和关键源码；
  fork 经 rebase 后不能把提交 SHA 变化当成新增功能，应结合两版相对各自上游的 diff、
  `git range-diff` 和编号改造记录判断净变化。相同源码重发时明确没有源码变化，
  构建配置变化也不能描述成产品功能；实验性能力标明适用条件。
- 用中文概括具体能力、修复和行为影响，显著标记 Breaking change 并写迁移方法；
  不复制完整提交列表，也不把已有功能重新列为本版新增。Windows 等未提供平台包的变化需标明范围。
- 在正文写明前版与本版及比较链接；上游和 fork 基线分别链接，rebase 导致比较混入历史时说明限制，
  前版 tag 已删除时使用已核实的完整 commit SHA 链接。完整源码比较仅作追溯入口，不代替语义摘要。
- 写入前重新读取 Release，核对 tag、源码和现有正文；正文放入临时 Markdown 文件，
  用 `gh release edit --notes-file` 更新。回读核对正文及 Release 状态、tag、资源未被改变。
  不重建已有 Release，不改 tag、产物或发布时间；重复执行时更新原差异说明，不重复追加。
