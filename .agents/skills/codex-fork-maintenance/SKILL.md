---
name: codex-fork-maintenance
description: 维护 happy-proto/codex 个人 fork；用于判断修改所属功能层、维护 GitHub PR stack、同步上游 alpha、验证 fork 集成版本、发布或安装 fork CLI。
---

# Codex Fork Maintenance

## 固定约定

- `origin` 指向 `happy-proto/codex`，`upstream` 指向 `openai/codex`。
- `main` 是选定上游 alpha Release 的原始提交，不是上游最新 main，也不包含 fork 修改。
- `fork` 是完整 stack 顶部及默认分支。PR 长期开启；不合并进 main。
- 每层表示独立的维护目的，可以依赖前一层。修复、测试和文档归属原功能层；新维护目的另建一层。
- 先读取实时 refs、PR base/head 和 stack，不硬编码 PR 编号或层数。
- 用户发起 alpha 同步；Actions 不自行 rebase 或强推 stack。
- 首期产物为 macOS Apple Silicon CLI 完整包，ad-hoc 签名、不公证。不新增 i18n。
- 本机性能有限：本地只执行轻量格式、静态检查、定向脚本测试和已构建产物验收。编译、release 构建、全量或其它重型测试交给 GitHub Actions；未经用户明确要求不在本地执行。
- 提交、推送、历史改写、平台更新和本机安装遵循当前任务授权；维护 skill 本身不扩大授权。

## 按需读取

- 修改层、交付代码及调整 stack：读 [stack.md](references/stack.md)。
- 同步 alpha 或分析上游变化：读 [upstream.md](references/upstream.md)。
- 构建、发布、安装、更新和回退：读 [release.md](references/release.md)。
