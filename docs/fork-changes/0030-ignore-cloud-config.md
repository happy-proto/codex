# 0030 按项忽略 Cloud config

## 功能

支持按项忽略可能被错误下发的 Cloud config 配置。所有开关默认关闭。

在用户级 `config.toml` 中配置：

```toml
[ignore_cloud_config]
models = true
fast_mode = false
ultrafast_mode = false
chronicle = true
```

- `models`：忽略工作区模型策略，包括新会话的模型、推理强度和服务档位。
- `fast_mode`：忽略 Fast mode 访问策略。
- `ultrafast_mode`：忽略 Ultra Fast mode 访问策略。
- `chronicle`：忽略 Computer History 访问策略。

`true` 只忽略对应策略，不直接启用功能。例如启用 Computer History 仍需配置
`[features]` 下的 `chronicle = true`。服务端权限校验不受影响。

支持 `-c ignore_cloud_config.models=true` 等命令行覆盖；项目配置和云端配置不能控制这些开关。
独立的 requirements 查询读取用户级设置，不接受会话命令行覆盖。
设备 MDM、本地系统要求以及其它云端策略保持原有行为。

## 实现与维护

入口为 `codex-rs/config/src/config_toml.rs` 的 `IgnoreCloudConfig`，
完整配置与独立 requirements 加载均在组合前过滤云端策略片段。
按稳定策略 ID 匹配，同时适用于普通配置和 requirements；不修改原始云端缓存。
未知策略继续加载。上游调整策略 ID 或拆分策略时需要核对对应关系。
旧版 `rbac-fast-mode` 可能同时限制两个加速档位，忽略该片段也会忽略其兼容性扩展限制；
单独下发的 Ultra Fast mode 策略仍由自己的开关控制。

## 验证

定向 Rust 测试覆盖默认保持原样、四项独立选择、无关片段保留、命令行覆盖、
完整配置与独立 requirements 加载一致性。Rust 测试需在 CI 中执行，本机只做格式与静态检查。
