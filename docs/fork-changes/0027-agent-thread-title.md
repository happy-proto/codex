# 首轮由 Agent 主动命名会话

## 动机与行为

用户的首条输入可能只有文件或消息链接，收到输入就生成标题容易停留在泛化描述。
等待首轮结束又会让十几分钟的长任务一直没有可辨识的名称。

新建的本地或远程 app-server 会话注入首轮提示词，要求 Agent 理解任务后立即调用
`codex_tui` 的 `set_thread_title`，随后继续工作。需要读取文件、消息或 URL 时，
先读到足以识别真实任务的内容，再命名，不等待整个任务或回合结束。
标题使用用户语言，保持简短、具体，并保留工单编号。

初次命名调用省略 `threadId`，并传入 `onlyIfUnnamed: true`。
工具读取当前会话名称，已有名称时返回原名称而不写入；默认的显式改名仍允许覆盖。
名称持久化及界面更新复用现有 `thread/name/set` 与名称通知。

移除用户消息和首轮完成时的自动标题请求，不再为初次命名创建独立的模型会话。
提示词只在新建会话时加入，恢复、fork、临时会话和云会话不额外注入。
`/rename` 的手动标题建议保留上游行为。

这是一项模型提示约定，不强制工具调用。如果模型没有遵循提示，或改名工具不可用，
本轮仍可正常执行，但不会再通过首轮结束后的独立总结补出标题。

## 实现入口与验证

- [thread_title_instructions.rs](../../codex-rs/tui/src/thread_title_instructions.rs)：首轮提示词及注入范围。
- [app_server_session.rs](../../codex-rs/tui/src/app_server_session.rs)：新建会话时合并提示词，保留已有开发者指令。
- [dynamic_tools.rs](../../codex-rs/tui/src/dynamic_tools.rs)：初次命名的已有名称检查。
- [dynamic_tools_tests.rs](../../codex-rs/tui/src/dynamic_tools_tests.rs)：真实内嵌 app-server 配合模拟模型，验证首轮仍在等待工具结果时名称已持久化，且命名不额外请求模型；覆盖重复命名和手动名称优先。
- [thread_title_tests.rs](../../codex-rs/tui/src/app/thread_title_tests.rs)：首轮完成不再启动隐藏标题请求，保留手动命名与取消回归。

定向测试通过 [fork CI](../../.github/workflows/fork.yml) 的 `thread_title` 过滤器执行。
同步上游时关注新建会话的指令合并、任务工具注册和名称通知；不修改 app-server 协议。
