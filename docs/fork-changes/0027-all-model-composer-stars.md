# 0029：为所有模型开启输入框星光

状态：维护中。

## 行为与配置

将上游仅面向 Astra 的输入框星光直接扩展到所有模型，包括 Sol、Luna 和自定义模型。
不新增配置项，仍使用上游的动画和星光总开关：

```toml
[tui]
animations = true

[tui.effects]
starfield = true
```

模型适用范围在代码中固定，不提供独立的范围开关。
仍需开启动画和星光总开关。确认新建空会话或真实回合完成后，
聚焦的空白输入框持续显示星光，不再在十五秒后淡出。
输入内容、附件、任务运行、搜索和弹窗期间隐藏，清空草稿后恢复；
失焦时停止绘制及调度动画帧。恢复历史会话或回放完成通知本身不启动效果，
但该会话随后真实完成一轮后同样适用。

## 入口与验证

- `tui/src/bottom_pane/chat_composer/sparkle.rs`：适用模型判断。
- `tui/src/chatwidget/session_flow.rs`：确认模型选择后的效果入口。
- `tui/src/chatwidget/protocol.rs`：真实回合结束后恢复空闲效果，不覆盖自动续跑状态。

`workspace_starfield_is_available_for_every_model_and_respects_motion_switches`
覆盖 Astra、Sol、Luna、自定义模型以及动画和星光总开关。
`sparkle_submission_tests` 覆盖连续回合、回放、草稿保留与清空恢复。
同步上游时保留输入跟踪、焦点及原始星光绘制规则，核对真实回合与回放入口的区别。
