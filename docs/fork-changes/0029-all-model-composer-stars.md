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
仍需开启动画和星光总开关；
不改变仅在新建且尚未输入的任务显示、输入后结束、最多十五秒淡出的规则。
恢复历史会话或打开 side 不会重新触发效果。

## 入口与验证

- `tui/src/bottom_pane/chat_composer/sparkle.rs`：适用模型判断。
- `tui/src/chatwidget/session_flow.rs`：确认模型选择后的效果入口。

`workspace_starfield_is_available_for_every_model_and_respects_motion_switches`
覆盖 Astra、Sol、Luna、自定义模型以及动画和星光总开关。
同步上游时保留输入跟踪、焦点及原始星光绘制规则，只扩展模型判断。
