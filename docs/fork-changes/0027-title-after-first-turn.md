# 首轮完成后生成会话标题

## 动机与行为

用户的首条输入可能只有文件或消息链接。上游在用户消息完成时立即生成标题，
此时 Agent 尚未读取引用内容，标题容易停留在“查看文件”等泛化描述。

自动标题改为在首轮 `turn/completed` 通知到达后生成，使用该轮用户请求和 Agent 的
实质答复，让已查明的任务内容参与总结。收到用户消息、工具调用或中途 commentary
时不启动标题请求；失败、中断、没有实质答复的回合不触发，也不因后续回合完成而重新命名。

标题仍通过后台临时会话生成，不阻塞正常对话。切换到其它会话后，原会话首轮完成
仍可触发命名；结果只应用到原会话。已有名称优先，手动改名取消待处理请求，
结果保存前仍检查服务端名称。云会话的服务端命名和 `/rename` 的手动建议保持上游行为。

## 实现入口

- [app_server_events.rs](../../codex-rs/tui/src/app/app_server_events.rs)：
  在接收线程通知、确认路由归属后检查首轮完成，覆盖前台和后台会话。
- [thread_title.rs](../../codex-rs/tui/src/app/thread_title.rs)：
  从完成通知、已保存 turns 和该轮已完成的消息中收集请求及最终答复，按 item ID 去重，
  沿用已有的长度限制、Unicode 边界与转义规则，忽略 commentary 和工具输出。
- [thread_routing.rs](../../codex-rs/tui/src/app/thread_routing.rs)：
  移除收到用户消息后立即生成标题的入口。

## 验证与维护

[标题回归](../../codex-rs/tui/src/app/thread_title_tests.rs) 使用真实内嵌 app-server 和模拟
模型响应，检查用户输入、commentary、最终消息到达时尚未生成标题，首轮完成后才发起请求，
请求包含 Agent 从引用中查明的结果；同时覆盖重复通知、失败或中断、后续回合、
后台会话完成、手动改名取消和原会话名称优先。该测试组接入 [fork CI](../../.github/workflows/fork.yml)。

同步上游时关注标题生成入口、`TurnCompleted` 的 items、线程事件缓存和手动命名的取消机制。
本改造改变 TUI 发起标题请求的时机，不修改 app-server 协议或服务端的命名规则。
