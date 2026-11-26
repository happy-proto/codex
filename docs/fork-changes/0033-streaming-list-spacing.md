# 流式 Markdown 列表间距

自绘 TUI 会话中，含多行条目的 Markdown 列表在完成后会为同级条目统一留空。原先流式阶段使用紧凑列表，完成事件触发的重新渲染会补入多个空行，使已经读到的内容突然移动。

Agent 回复与计划预览在流式阶段沿用完成态的列表间距规则。最后一个含列表的 Markdown 块保留在可重绘区域，直到后续顶层块出现或消息完成。在预览大小限制内，尚未收到行尾的条目也使用当前列表的上下文排版。自绘历史队列保留已经适配窗口宽度的续行缩进。原生终端回滚区沿用其历史间距规则。

首个列表项尚未收到换行时，预览在大小限制内把前一块和新行交给同一个 Markdown 渲染器。前一块的渲染行匹配时，预览追加渲染器生成的分隔行和新内容。

- `codex-rs/tui/src/chatwidget/streaming.rs` 为自绘会话选择统一间距。
- `codex-rs/tui/src/markdown_render/streaming.rs` 追踪含列表的末尾块，流式控制器据此保留可重绘行。
- `codex-rs/tui/src/streaming/controller_preview_tests.rs` 覆盖短条目、后续换行条目与列表进入稳定队列后的输出。
- `codex-rs/tui/src/chatwidget/tests/list_spacing_tests.rs` 对照回复与计划在完成、被打断及窗口宽度变化时的排版。
- `scripts/fork/check_streaming_list_layout.py <已构建的 codex>` 启动隔离 PTY 和本地模拟 Responses API，比较首项出现、收到换行和回复完成后的终端行。平台包构建运行此验收。

同步上游时核对自绘会话的流式和历史渲染是否采用同一间距策略，以及含列表的末尾块是否可重新绘制。
