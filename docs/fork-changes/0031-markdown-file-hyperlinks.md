# 0031 Markdown 文件终端链接

状态：维护中。

## 为什么改

终端自动识别纯文字文件路径时，Codex 将长路径排成多行后，终端可能无法还原完整目标。
明确的 Markdown 文件链接已经提供目标，不必再次从显示文字推断。

## 如何改

为明确的本地 Markdown 文件链接的标签及显示路径附加 OSC 8，使用完整的 `file://` 目标。
保留原有标签、括号路径、配色和复制内容；换行后每个片段继续指向同一文件。
绝对路径直接转换，相对 `./`、`../` 路径使用会话工作目录，`~/` 使用本机主目录。
支持已有的 `file://` 目标和百分号编码；`:line:col` 与 `#Lline` 等位置后缀继续显示，
但不作为文件名或跳行承诺。普通文字和代码中的裸路径不自动识别。
跨操作系统路径无法在当前系统解释时保留原显示，不生成错误的本机目标。

## 入口和验证

- [目标解析](../../codex-rs/tui/src/markdown_render/local_links.rs)。
- [Markdown 渲染及表格](../../codex-rs/tui/src/markdown_render.rs)。
- [终端链接元数据与 OSC 8 输出](../../codex-rs/tui/src/terminal_hyperlinks.rs)。
- [渲染回归](../../codex-rs/tui/src/markdown_render_tests.rs)覆盖窄屏跨行、表格和裸路径边界；
  [目标回归](../../codex-rs/tui/src/markdown_render/local_links_tests.rs)覆盖编码和位置后缀。

## 兼容和上游同步

点击行为由终端及系统文件关联决定；远程会话的 `file://` 打开位置也由客户端处理。
通用网页链接仍只接受 HTTP(S)，文件链接由 Markdown 本地目标专用入口创建，
不扩大裸 URL 的协议识别范围。同步时保留链接类型、换行映射和表格回退中的完整目标。
