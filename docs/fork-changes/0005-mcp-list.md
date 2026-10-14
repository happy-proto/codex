# 0005 MCP 列表展示

## 为什么改

按传输类型挤在一张宽表中的输出不易阅读，长命令、环境变量和禁用原因需要清晰展示。

## 如何改

`codex mcp list` 默认按服务器显示信息块，将启用状态、传输方式、命令、参数和地址分行展示。
支持 `--format human|plain`，保留原有 `--json` 数据结构和认证探测。
终端按能力配色，重定向、`NO_COLOR` 或 `TERM=dumb` 时关闭配色。
长字段换行，环境变量值脱敏，控制字符转义。

## 入口和验证

- [展示实现](../../codex-rs/cli/src/mcp_list_output.rs)、[命令实现](../../codex-rs/cli/src/mcp_cmd.rs)。
- [CLI 回归](../../codex-rs/cli/tests/mcp_list.rs)、[命令测试](../../codex-rs/cli/src/mcp_cmd_tests.rs)。

## 兼容和上游同步

**Breaking change**：解析旧文本的脚本添加 `--format plain`；结构化读取继续使用 `--json`。
启用状态只是配置状态，不能描述成连接健康。
同步时保留脱敏和控制字符边界，避免新字段泄露凭据。
历史讨论：[PR #10](https://github.com/happy-proto/codex/pull/10)。
