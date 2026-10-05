# Stack 和修改归属

先核对 remote URL、`gh repo view happy-proto/codex`、远端 refs 和每层 PR 的标题、正文及相对直接 base 的净变化。
修复已有功能、补齐测试或文档留在拥有该目的的层；新的可独立撤销的需求另建功能层。
治理文档、构建发布和产品功能按实际维护目的区分，不能根据当前 checkout 猜归属。

使用官方 `github/gh-stack` 扩展；先逐层查看 `gh stack --help` 和目标子命令帮助。
所有 `gh stack` 命令明确使用 `GH_REPO=happy-proto/codex`，支持 `--remote` 的写命令同时指定 `origin`。
仅指定 `--remote origin` 仍可能让 PR 查询落到上游；看到上游 PR URL 或意外的 merged 提示时立即停止并重新核对路由。
扩展会从 origin 推断仓库身份，不能只信 GH_REPO；检查 `.git/gh-stack` 的 repository、REST URL 和 PR URL 都指向 fork。

修改下层后先级联 rebase 上层，再统一推送，保留 PR 编号和讨论。推送前记录远端旧 SHA；必要的历史改写用明确旧 SHA 的 force-with-lease。
不要推送到 upstream，不自动改变 draft 状态或合并、关闭 PR。

结构调整用 `gh stack modify` / `submit`。删除被上游吸收的层，先验证行为等价并重放上层；
GitHub 不允许修改仍在 stack 中的 PR base 时，先通过官方 stack 操作解除关联，再更新结构。
网络错误后回读平台状态，避免重复已成功的写操作。

提交前遵守仓库工具链，在本地做轻量格式和定向脚本检查；编译、Rust crate 测试和全量测试在 GitHub Actions 执行。
推送后回读所有 PR base/head、检查和 mergeability。
确认历史线性、本地与远端一致、`fork` 仍为顶部和默认分支。CI queued/running 不是通过。
长期 PR 是维护边界，不以合并它们作为完成条件。
