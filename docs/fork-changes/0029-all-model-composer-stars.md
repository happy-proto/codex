# 0029：为所有模型开启输入框星光

状态：维护中。

## 行为与配置

上游 `tui.effects.starfield` 仍是星光总开关，默认只对 Astra 生效。
新增范围配置可将同一效果用于 Sol、Luna 及其它模型：

```toml
[tui]
animations = true

[tui.effects]
starfield = true
starfield_all_models = true
```

`starfield_all_models` 默认关闭，以保留上游默认行为。
该配置只改变模型适用范围，仍需开启动画和星光总开关；
不改变仅在新建且尚未输入的任务显示、输入后结束、最多十五秒淡出的规则。
恢复历史会话或打开 side 不会重新触发效果。

## 入口与验证

- `config/src/tui_effects.rs` 及 `core/config.schema.json`：配置契约。
- `tui/src/bottom_pane/chat_composer/sparkle.rs`：适用模型判断。
- `tui/src/chatwidget/session_flow.rs`：确认模型选择后的效果入口。

`workspace_starfield_can_be_enabled_for_every_model_and_still_respects_motion_switches`
覆盖 Astra、Sol、Luna、自定义模型以及动画和星光总开关。
同步上游时保留输入跟踪、焦点及原始星光绘制规则，只扩展模型判断。
