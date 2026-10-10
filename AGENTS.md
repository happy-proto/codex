# Fork 开发

本仓库是个人 fork `happy-proto/codex`。修改 fork、维护 PR stack、同步上游 alpha、
发布或安装时，使用 [维护 skill](.agents/skills/codex-fork-maintenance/SKILL.md)。

- `origin` 指向 `happy-proto/codex`，`upstream` 指向 `openai/codex`。
- `main` 保持选定上游 alpha Release 的原始提交，不包含 fork 修改。
- `fork` 是完整 stack 的顶部及默认分支；功能 PR 长期开启，不合并进 `main`。
- 栈底保留一个长期杂项 PR，集中维护 fork 规范、构建、发布、安装、更新及相关小修正；
  独立产品功能在其上分别建层。
- 按维护目的确定修改归属：已有功能的修复、测试和文档留在原层，新需求单独建层；修改下层后重放依赖层。
- 保留上游许可证和声明，不新增 i18n；不为了方便 fork 维护而改变上游 API。
- 功能裁剪以实测构建收益和长期上游同步成本为依据，优先采用修改集中、维护成本低的方案。
  不为少量耗时收益大范围删除上游功能或增加复杂条件分支。
- 沿用 Rust/Cargo/just 工具链。本机性能有限，只做格式、静态检查、定向脚本测试和已构建产物验收。
  编译、release 构建、全量或其它重型测试交给 GitHub Actions，未经用户明确要求不在本地执行。
- 本 fork 的 PR 标题和正文用中文，commit message 用英文。

## 维护本文件与 skill

- 仓库通用规则集中在 `AGENTS.md`；维护 skill 引用本文件，只保留任务入口和具体操作方法，
  不重复已在本文件定义的规则，包括 skill 的参考文件。
- Fork 自己新增或修改的文档、维护说明使用中文，方便识别来源；上游原有说明保持英文。
  同一文件包含两者时保留清晰边界，不翻译未修改的上游内容。
