# 0006 Markdown 链接和列表主题配色

## 为什么改

Markdown 链接和有序列表编号原先使用固定颜色，与用户选择的语法主题不一致。
内置 Dracula 的链接配色也需要接近 Zed。

## 如何改

链接优先使用 Markdown 链接 scope，有序编号读取编号与标点 scope；缺失时保留原有回退。
保留下划线、加粗、斜体和 ANSI 终端调色板语义。
Dracula 的代码块链接文字为粉色、URL 为青色、括号为紫色，渲染后的可点击链接为青色。
自定义主题使用 `markup.underline.link.markdown` 或 `markup.link.markdown`，
列表编号使用 `markup.list.numbered.markdown`。

## 入口和验证

- [渲染](../../codex-rs/tui/src/markdown_render.rs)、[高亮](../../codex-rs/tui/src/render/highlight.rs)。
- [Dracula 覆盖](../../codex-rs/tui/assets/themes/dracula-markdown.tmTheme)、[链接回归](../../codex-rs/tui/src/markdown_render/web_links_tests.rs)。

## 上游同步

核对主题 scope 匹配、加载优先级和 ANSI 回退；完整与流式输出须一致。
上游关联：[Issue #47497](https://github.com/openai/codex/issues/47497)；历史讨论：[PR #17](https://github.com/happy-proto/codex/pull/17)。
