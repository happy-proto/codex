# 同步上游 alpha

维护 fork 的改动默认包含同步最新已发布的上游 alpha，无需用户另行提出同步。
只读调查不改变基线；用户明确要求保留基线、暂不同步或指定版本时，遵循该要求。
仅限本地修改的任务也只在本地准备同步；提交、推送、发布和安装仍遵循当前任务的授权范围。
Actions 不自行 rebase 或强推分支。

本轮开始时确定已发布的候选；`scripts/fork/upstream-tag` 已对应该候选时，报告已是最新并跳过 alpha rebase，
仍须完成下面的内置 skill 更新检查。
将本轮改动与上游同步都准备、验证完毕后，一次推送 `main`，避免重复触发完整 CI。
本轮确定候选后不反复追逐新发行版。

## 内置第三方 skills

每次同步上游时执行 `python3 scripts/fork/sync_skills.py check --upstream`，
按 `scripts/fork/skills.lock.json` 中各来源的跟踪 ref 检查更新；检查失败不能视为没有更新。
有更新时，用 `python3 scripts/fork/sync_skills.py update <skill> --ref <ref>` 更新对应快照和 lock，
保留原有跟踪 ref，避免改为固定 commit 后失去后续更新发现能力。
审查 skill 指令、引用文件和许可证差异，确认仍适用于 fork 的接入方式及运行程序；
执行离线 `check` 和 `test_sync_skills.py`，将快照及 lock 与 alpha 同步结果一起验证、交付。
来源及维护边界见 [0010 改造记录](../../../../docs/fork-changes/0010-open-automation.md)。
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
   回读默认分支、远端 SHA 与 CI，清理临时 worktree。

同步完成后的最终回复必须简要列举本次引入的主要上游变更，不能只报告已同步或 CI 状态。
以上一次记录的上游 tag 为基线，先写新旧 alpha 版本，再按功能与维护主题列出对用户有意义的变化；
说明具体新增能力、修复或行为变化，不用文件清单、提交标题堆砌或只有提交数量代替说明。
另行区分纯 rebase 和实际 fork 语义适配，说明各改造点的变化、验证、最新 SHA 的 CI/Release 状态、
待决事项和未执行的动作。摘要保持简短；只有高影响变化才展开细节。
自动发布成功不等于本机已经更新；安装是单独的验收步骤。
