# 0034：在回复下方展开回合详情

状态：维护中。

全屏 TUI 将 `Worked for…` 保留在最终回复后。有可折叠的工具调用、进度消息或推理摘要时，
在耗时行加箭头，可点击或通过活动导航键展开。缺少耗时元数据的旧回合使用中间消息计数作为入口。
同一条用户消息下的多个回合，各自使用对应的完成记录。

`tui/src/transcript_view/turn_details.rs` 负责把中间过程关联到对应完成行。
`layout.rs` 和 `disclosure.rs` 负责绘制与交互。完成记录沿用上游格式，详情展开后仍按原顺序显示。
分栏最新回合若有展开入口，由正文绘制该完成行，避免与输入区的固定状态重复。

默认分支 CI 运行 `tool_groups_expand_independently_inside_a_completed_turn` 和
`legacy_progress_stays_visible_until_a_final_answer`，覆盖耗时位置、鼠标与键盘操作、嵌套工具详情和旧历史。
同步上游时对照 #60557，核对折叠入口、完成元数据、搜索和分栏状态之间的关系。
