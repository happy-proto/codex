# 首轮由 Agent 主动命名会话

## 动机与行为

用户的首条输入可能只有文件或消息链接，收到输入就生成标题容易停留在泛化描述。
等待首轮结束又会让十几分钟的长任务一直没有可辨识的名称。

首轮通过中文提示词引导 Agent 在基本调研后主动命名会话，具体措辞以
[thread_title_instructions.rs](../../codex-rs/tui/src/thread_title_instructions.rs) 为准。

初次命名调用省略 `threadId`，并传入 `onlyIfUnnamed: true`。
工具读取当前会话名称，已有名称时返回原名称而不写入；默认的显式改名仍允许覆盖。
名称持久化及界面更新复用现有 `thread/name/set` 与名称通知。

提示词通过现有 `turn/start.additionalContext` 的应用上下文传入，成为紧靠用户输入前的
独立 developer message，不拼接进用户文本或既有开发者指令，不修改公共指令前缀。
后续 turn 不重复注入；已包含历史的恢复会话、fork、临时会话和云会话不注入。
这条消息作为正常历史保留，不在回合结束后删除或重新加入公共指令。

提前命名是一项模型提示约定，不强制工具调用。如果首轮成功完成且已有实质答复时仍未命名，
沿用既有隐藏会话，根据用户请求与最终答复补生成标题。启动兜底请求前在后台读取服务端名称，
即使改名通知尚未到达界面，也不为已经保存的标题再请求模型；结果保存前仍检查名称。
兜底标题保存复用上游后台写入器，不在 TUI 事件循环中等待服务端；手动命名保留优先权。
失败、中断、没有实质答复及后续回合不触发兜底。`/rename` 的手动标题建议保留上游行为。

## 实现入口与验证

- [thread_title_instructions.rs](../../codex-rs/tui/src/thread_title_instructions.rs)：首轮提示词及应用上下文。
- [user_turn_submission.rs](../../codex-rs/tui/src/app/user_turn_submission.rs)：首次用户 turn 提交时附带上下文。
- [user_message_submission.rs](../../codex-rs/tui/src/app_server_session/user_message_submission.rs)：统一消息提交流程通过 `TurnStartSettings` 透传上下文，仅新 turn 使用；保留提交回执、未确认草稿及 steer 行为。
- [app_server_session.rs](../../codex-rs/tui/src/app_server_session.rs)：通过既有协议转发上下文，公共指令保持不变。
- [dynamic_tools.rs](../../codex-rs/tui/src/dynamic_tools.rs)：初次命名的已有名称检查。
- [dynamic_tools_tests.rs](../../codex-rs/tui/src/dynamic_tools_tests.rs)：真实内嵌 app-server 配合模拟模型，验证独立消息紧靠用户输入、首轮等待工具结果时名称已持久化；覆盖重复命名和手动名称优先。
- [thread_title_tests.rs](../../codex-rs/tui/src/app/thread_title_tests.rs)：首轮注入范围、未命名兜底、已有服务端名称跳过兜底，以及原有手动命名与取消回归。

定向测试通过 [fork CI](../../.github/workflows/fork.yml) 的 `thread_title` 过滤器执行。
同步上游时关注新建会话的指令合并、任务工具注册和名称通知；不修改 app-server 协议。
