# 通过 @ 选择插件提供的 skill

## 动机与行为

中文输入法下输入 `$` 可能得到人民币符号，需要切换输入法才能调用 skill。
上游统一 `@` 菜单虽然支持 skill，但会隐藏所属插件同时出现在菜单中的 skill，
导致通过插件安装的 skill 仍需从 `$` 菜单选择。

默认启用的 `mentions_v2` 菜单列出全部已加载 skill，包括插件提供的 skill。
输入 `@` 后搜索 skill 名称或显示名称，按 Enter 或 Tab 即可选择；
CLI 自动填入标准的 `$skill名`，并绑定该 skill 的 `SKILL.md` 路径。
例如输入 `@deploy`，可以选择插件的 `sample:deploy` skill，填入 `$sample:deploy`。

插件自身仍作为独立条目保留，`$` 菜单也保持可用。
手动关闭 `mentions_v2` 时沿用上游的文件专用 `@` 菜单。
本改造只增加补全候选，不把未从菜单选择的任意 `@name` 文本解释为 skill 调用。

## 实现入口

- [search_catalog.rs](../../codex-rs/tui/src/bottom_pane/mentions_v2/search_catalog.rs)：
  不再按 `plugin_id` 隐藏 skill；保留原有的 skill 插入文本和路径绑定。
- [chat_composer.rs](../../codex-rs/tui/src/bottom_pane/chat_composer.rs)：
  沿用统一菜单的选择、草稿和提交逻辑，不修改服务端 skill 调用协议。
- [mention_codec.rs](../../codex-rs/tui/src/mention_codec.rs)：
  引用名称允许插件 skill 的 `namespace:name` 格式，选择后及历史编解码均保留完整名称和路径。

## 验证与维护

`plugin_skill_completion` 回归测试覆盖 `@` 和 `$`、Enter 和 Tab、
skill 与插件两种元数据到达顺序，以及选择后提交时保留正确路径绑定；
同时检查仍能选择插件本身；`mention_codec::tests` 检查命名空间名称的历史编解码。
两组测试接入 [fork CI](../../.github/workflows/fork.yml)。
同步上游时关注统一菜单的候选构建、skill 路径绑定和 `mentions_v2` 的默认行为。
若上游已允许插件与其 skill 同时显示，应移除重复差异并保留本记录。
