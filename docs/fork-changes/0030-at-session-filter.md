# @ 菜单的会话筛选

## 动机与行为

统一 `@` 菜单已有会话候选，但此前只能在 All Results 中查找。
新增 Sessions 类别，排列在 Plugins 后，只展示现有会话候选；
左右键在 All Results、Filesystem Only、Plugins、Sessions 之间循环。
Plugins 的名称和包含插件、skill 的行为保持不变。

Sessions 沿用已有的标题、工作目录和摘要搜索，以及 `thread://` 引用绑定；
不改变会话来源、排序、引用上限或提交协议。

## 实现与验证

- `mentions_v2/search_mode.rs` 定义筛选与双向循环，测试覆盖类别隔离和导航。
- `mentions_v2/render.rs` 增加标签；渲染测试覆盖窄窗口下选中标签可见。
- `mentions_v2/popup.rs` 测试会话搜索、空结果恢复和引用绑定保留。
- fork CI 执行 `bottom_pane::mentions_v2::` 测试，包括渲染快照。

同步上游时关注 SearchMode、标签列表以及 MentionType::Task 的映射；
上游提供等效会话筛选后移除重复实现并保留本记录。
