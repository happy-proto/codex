# 0007 工作中显式 skill 正文注入

状态：维护中。

## 为什么改

steer 中显式调用 skill 时，补充消息进入上下文，但 skill 正文可能没有加载。
设置 `allow_implicit_invocation: false` 后，模型也不能从隐式目录补找正文。

## 如何改

初始输入与 steer 共用显式 skill 解析、加载、依赖处理和调用事件逻辑。
文本 `$skill` 和结构化附件在下一次模型请求前注入正文；同批消息沿用去重。
只处理被 hook 接受的消息，保留禁用、隐式过滤和 Guardian 隔离，
不重跑 plugin 或 extension 的回合启动流程。

## 入口和验证

- [采样和 steer 入口](../../codex-rs/core/src/session/turn.rs)、[skill 集成测试](../../codex-rs/core/tests/suite/skills.rs)。
- 历史 red/green：旧实现的 4 个注入用例失败、3 个禁用和未调用用例通过；修复后 7 个用例通过。
  [旧实现 CI](https://github.com/happy-proto/codex/actions/runs/38072026160)、[修复 CI](https://github.com/happy-proto/codex/actions/runs/38071992396)。

## 上游同步

只修复 steer 路径遗漏，不扩展到 skill 名称大小写、点号或 compaction 后调用状态。
测试保留流式响应门控，不能仅证明提示里出现 skill 名称。
上游关联：[Issue #23454](https://github.com/openai/codex/issues/23454)、[Issue #47011](https://github.com/openai/codex/issues/47011)；历史讨论：[PR #19](https://github.com/happy-proto/codex/pull/19)。
