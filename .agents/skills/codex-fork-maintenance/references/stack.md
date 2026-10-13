# Stack 和修改归属

先核对 remote URL、`gh repo view happy-proto/codex`、远端 refs 和每层 PR 的标题、正文及相对直接 base 的净变化。
文案描述相对直接 base 的最终净变化；不根据当前 checkout 猜修改归属，不硬编码 PR 编号或层数。

使用官方 `github/gh-stack` 扩展；先逐层查看 `gh stack --help` 和目标子命令帮助。
所有 `gh stack` 命令明确使用 `GH_REPO=happy-proto/codex`，支持 `--remote` 的写命令同时指定 `origin`。
仅指定 `--remote origin` 仍可能让 PR 查询落到上游；看到上游 PR URL 或意外的 merged 提示时立即停止并重新核对路由。
扩展会从 origin 推断仓库身份，不能只信 GH_REPO；检查 `.git/gh-stack` 的 repository、REST URL 和 PR URL 都指向 fork。

用级联 rebase 更新依赖层后统一推送，保留 PR 编号和讨论。推送前记录远端旧 SHA；必要的历史改写用明确旧 SHA 的 force-with-lease。
不要推送到 upstream，不自动改变 draft 状态或合并、关闭 PR。

## 整理每层提交

提交数量与例外遵循仓库 [AGENTS.md](../../../../AGENTS.md)。
用 `git rev-list --count <direct-base>..<head>` 统计每层独有提交，
在本轮改动、上游同步和依赖层重放完成后、统一推送前检查。

从栈底向上处理，先保存各层旧 head、直接 base 和远端期望 SHA。
更新现有单提交层时，在授权范围内 amend；整理多提交层时，用明确的直接 base 边界合并该层独有提交，
不要把继承的下层提交一起 squash。英文 Conventional Commit 概括整层最终净变化，
保留必要的作者归属和 breaking-change 信息。
整理一层后，重放其依赖层，再按新的直接 base 检查各层提交数。
逐层确认压缩前后最终 Git tree 完全相同，PR 的职责和净变化不变；
完成整个 stack 后，用明确旧 SHA 的 force-with-lease 统一推送并回读远端提交数。
检查多提交 PR 的正文是否说明保留原因；`Fork PR checks` 的提示不能代替这项检查。
这项历史整理不改变任务原有的提交、推送或安装授权范围。

结构调整用 `gh stack modify` / `submit`。删除被上游吸收的层，先验证行为等价并重放上层；
GitHub 不允许修改仍在 stack 中的 PR base 时，先通过官方 stack 操作解除关联，再更新结构。
网络错误后回读平台状态，避免重复已成功的写操作。

推送后回读所有 PR base/head、检查和 mergeability。
确认历史线性、本地与远端一致、`fork` 仍为顶部和默认分支。CI queued/running 不是通过。
