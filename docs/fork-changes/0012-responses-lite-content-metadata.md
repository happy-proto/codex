# 0012 Responses Lite 请求来源字段兼容

状态：维护中。

## 为什么改

上游客户端在 Responses Lite 请求中发送
`internal_chat_message_metadata_passthrough.content_item_metadata`，
官方后端会以未知参数拒收，导致模型尚未生成就返回 400。
问题见 [上游 issue #52313](https://github.com/openai/codex/issues/52313)；
[PR #53831](https://github.com/openai/codex/pull/53831) 重新引入了该字段，
`rust-v0.164.0-alpha.1` 包含此实现。

关闭稳定功能 `content_item_kinds` 可以绕过错误，但也丢失请求侧内容分类。
仅清理被拒收的来源字段即可保留分类，不必改变功能默认值或逐台修改用户配置。

## 如何改

- 在 HTTP/SSE 与 WebSocket 共用的请求构造入口，对出站输入副本调用已有的
  `clear_content_item_metadata()`；Responses Lite 与普通 Responses 请求都执行清理。
- 保留 `content_item_kinds`、`turn_id` 等后端接受的元数据。稳定功能仍按上游默认开启，
  用户显式关闭时继续遵循已有行为，不新增兼容开关。
- 普通 Responses 原本就清理此字段，第三方 provider 原本就清理整个内部元数据；
  两者行为不变，不修改协议类型或字段定义。
- 清理只作用于请求副本。本地历史、rollout 和压缩保留记录继续保存分类与来源信息，
  不改变依赖这些记录的来源分析。远程压缩 v2 复用同一 model client，受同样的出站清理保护。

## 入口和验证

- [请求构造](../../codex-rs/core/src/client.rs)：`build_responses_request`。
- [HTTP/SSE 测试](../../codex-rs/core/tests/suite/responses_lite.rs)：新建与续接会话的
  请求不含来源字段，基础指令仍带内容分类。
- [WebSocket 测试](../../codex-rs/core/tests/suite/client_websockets.rs)：普通与 Responses Lite
  模型、官方与自定义 provider 的请求契约；官方请求保留分类和回合标识，原始 prompt 不变。
- [远程压缩测试](../../codex-rs/core/tests/suite/compact_remote.rs)：续接历史后执行 v2 压缩，
  压缩及后续请求清理来源字段，本地压缩保留记录仍有用户来源与分类。
- 上述定向 Rust 测试由[自有 CI](../../.github/workflows/fork.yml)执行。
  本地不编译 Rust，使用官方二进制与临时出站过滤验证后端兼容性：保持
  `content_item_kinds` 开启，原始请求复现 400，仅移除来源字段后 HTTP/SSE 和 WebSocket
  都返回正常回复，本地 rollout 仍保留来源字段。此验证不替代补丁产物的安装验收。

## 上游同步

关注 issue #52313 及 #53831 的后续修复。上游回退字段或官方后端正式接受该 schema 后，
重新验证两种传输、新建与续接会话及远程压缩，再删除无必要的 fork 清理与对应断言。
不要通过删除本地来源数据或长期关闭稳定功能维持兼容。
