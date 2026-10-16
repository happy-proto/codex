# 0003 中文 Markdown 加粗边界

状态：维护中。

## 为什么改

中文及全角标点旁的双星号加粗会暴露原始星号，也可能与后续标记错误配对。
例如 `**加粗内容。**后续中文` 应按用户意图显示。

## 如何改

在 Markdown 解析与渲染之间重新配对受影响的中文双星号，保留源字节位置。
覆盖段落、标题、表格，完整输出、流式输出和选择复制使用一致处理；
保护代码、转义、实体、HTML 和链接边界，下划线和三重星号沿用原语义。
原始消息及整段原文复制不重写。

## 入口和验证

- [适配器](../../codex-rs/tui/src/markdown_render/cjk_strong.rs)、[渲染入口](../../codex-rs/tui/src/markdown_render.rs)。
- [回归测试](../../codex-rs/tui/src/markdown_render_tests.rs)、[流式处理](../../codex-rs/tui/src/markdown_render/streaming.rs)。

## 上游同步

仅覆盖 fork CLI，不改变官方 Desktop 或存储内容。
关注 Markdown parser、流式合并和源位置映射变化；上游吸收时用同一组保护边界和复制测试确认等价。
上游关联：[Issue #37531](https://github.com/openai/codex/issues/37531)；历史讨论：[PR #7](https://github.com/happy-proto/codex/pull/7)。
