# Stack 和修改归属

先核对 remote URL、`gh repo view happy-proto/codex`、远端 refs 和每层 PR 的标题、正文及相对直接 base 的净变化。
文案描述相对直接 base 的最终净变化；不根据当前 checkout 猜修改归属，不硬编码 PR 编号或层数。

使用官方 `github/gh-stack` 扩展；先逐层查看 `gh stack --help` 和目标子命令帮助。
所有 `gh stack` 命令明确使用 `GH_REPO=happy-proto/codex`，支持 `--remote` 的写命令同时指定 `origin`。
仅指定 `--remote origin` 仍可能让 PR 查询落到上游；看到上游 PR URL 或意外的 merged 提示时立即停止并重新核对路由。
扩展会从 origin 推断仓库身份，不能只信 GH_REPO；检查 `.git/gh-stack` 的 repository、REST URL 和 PR URL 都指向 fork。

用级联 rebase 更新依赖层后统一推送，保留 PR 编号和讨论。推送前记录远端旧 SHA；必要的历史改写用明确旧 SHA 的 force-with-lease。
不要推送到 upstream，不自动改变 draft 状态或合并、关闭 PR。

## 每层提交上限

每个 PR 相对其直接 base 的独有提交最多 5 个，统计时不计入下层继承的提交。
本轮改动、上游同步和依赖层重放完成后，在统一推送前检查所有层。
不超过 5 个时保持原样；有 K 个且 K > 5 时，将最早的 K − 4 个合成一个，
保留最新 4 个的顺序、提交说明和改动，最终恰好 5 个；不要把整层全部 squash。

从栈底向上处理，先保存各层旧 head、直接 base 和远端期望 SHA。
合并提交用英文 Conventional Commit 概括被合并部分的净变化，并保留必要的作者归属和 breaking-change 信息。
压缩一层后重放其依赖层，再按新的直接 base 检查各层提交数。
逐层确认压缩前后最终 Git tree 完全相同，PR 的职责和净变化不变；
完成整个 stack 后，用明确旧 SHA 的 force-with-lease 统一推送并回读远端提交数。
这项历史整理不改变任务原有的提交、推送或安装授权范围。

结构调整用 `gh stack modify` / `submit`。删除被上游吸收的层，先验证行为等价并重放上层；
GitHub 不允许修改仍在 stack 中的 PR base 时，先通过官方 stack 操作解除关联，再更新结构。
网络错误后回读平台状态，避免重复已成功的写操作。

推送后回读所有 PR base/head、检查和 mergeability。
确认历史线性、本地与远端一致、`fork` 仍为顶部和默认分支。CI queued/running 不是通过。
