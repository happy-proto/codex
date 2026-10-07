# 集成分支

`fork` 是长期 PR stack 的集成顶部及默认分支，包含仓库维护、alpha 发布与更新支持，
以及所有启用的功能层。推送后构建完整产品，验证通过才发布。

新功能插入在集成层下方，各功能分支和 PR 保留独立的维护边界；集成 PR 不合并进 `main`。
当前 stack 和上游 alpha 基线通过仓库内的维护 skill 核对，不从 PR 编号或上游最新开发提交推断。

## 功能层

- [中文 Markdown 加粗边界](https://github.com/openai/codex/issues/37531)：
  独立维护 CLI 渲染层对中文标点边界双星号的兼容，覆盖流式输出、完整回复和选择复制。
  不修改存储的原始消息，也不覆盖官方 Desktop 的渲染器。

## 后续维护

- [sccache 稳定版修复与产物缓存](https://github.com/happy-proto/codex/issues/6)：
  等待包含 metadata 流式通知修复的稳定版，按 Issue 的验收清单比较冷缓存、热缓存和原生编译。
  确认收益后再默认启用产物缓存。
