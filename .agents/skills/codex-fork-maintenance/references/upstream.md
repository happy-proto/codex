# 同步上游 alpha

由用户发起同步，Actions 不自行 rebase 或强推 stack。

1. 确認工作树、授权和 live stack；fetch origin、upstream 及候选 alpha tag，记录旧 main、每层 head 和远端期望 SHA。
2. 从 `openai/codex` 的已发布、非 draft alpha Releases 中选择版本最高的候选，解析 annotated tag 到真实 commit。
   不用 `/releases/latest`（它不包含 alpha），不直接以 upstream/main 作为同步目标。
3. 核对旧 main 是之前选定 alpha 的原始提交。记录新旧 tag、SHA、祖先关系、提交主题和高影响 diff。
   alpha tag 间可能不是快进；分叉时先查明上游发行历史，不把所有旧上游提交重放成 fork 修改。
4. 在临时 worktree 演练，从下到上用明确边界执行
   `git rebase --onto <new-direct-base> <old-direct-base> <layer>`，每层只重放自己的提交。
5. 对每层检查目的是否已被上游吸收、重构是否要求语义适配、新入口是否需要覆盖、生成物和对外契约是否仍一致。
   不机械选 ours/theirs，也不因为无文本冲突就跳过语义检查。移除或折叠功能层先明确授权。
6. 验证通过后在授权范围内更新 main 与整个 stack。
   更新发布层记录的上游 tag，使其与 main 一致；按 stack 文档检查和发布远端状态。

同步报告以旧 fork main 为基线，按功能与维护主题说明上游变化；区分纯 rebase 和实际语义适配，
说明每层净变化、验证、最新 SHA 的 CI/Release 状态、待决事项和未执行的动作。
自动发布成功不等于本机已经更新；安装是单独的验收步骤。
