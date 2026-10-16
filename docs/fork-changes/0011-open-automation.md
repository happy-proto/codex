# 0011 默认使用开放的浏览器与电脑操作工具

状态：维护中。

## 为什么改

自签名 fork 无法使用依赖官方 Desktop 运行环境的 browser/computer use。
CLI 与官方 Desktop 共用配置，直接禁用或卸载官方插件会影响 Desktop；
仅在提示词里建议替代工具也不能阻止官方工具加载。

## 如何改

- fork 在合并本地配置和远端安装状态后，过滤官方 marketplace 中的
  `chrome`、`browser`、`browser-use`、`computer-use` 和 `unified-computer-use`。
  对应的 skills、MCP、apps 和 hooks 不参与本次运行，也不作为模型的工具建议或远端推荐。
  同名第三方插件不受影响。不修改共享配置、官方插件缓存、安装及账号启用状态。
- 内置 [Open Browser Use](https://github.com/iFurySt/open-browser-use) 和
  [Open Computer Use](https://github.com/iFurySt/open-codex-computer-use) 的完整 skill 目录和许可证。
  上游原文不打补丁；快照随二进制构建，无需首次启动联网或初始化 submodule。
- 全部内置 system skills 使用 `$CODEX_HOME/.fork/skills`，与官方 Desktop 的
  `$CODEX_HOME/skills/.system` 分离。已有用户或项目同名开放工具 skill 优先于内置副本。
  保留 `skills.bundled.enabled = false` 对内置 skills 的控制。
- 浏览器通过 skill 指引使用 `open-browser-use` / `obu` CLI 或 SDK。
  不自动注册浏览器 MCP，以便每次任务显式选择浏览器、profile 和唯一 session id。
- 最终 MCP catalog 中没有已有 computer use 服务时，检测 PATH 上的
  `open-computer-use` 或 `ocu`，注册 `open_computer_use` 默认 stdio MCP（参数 `mcp`）。
  已有同名、标准别名或直接启动同一程序的配置，包括禁用项和插件提供的服务，均优先。
  同时识别 `npx` / `bunx` 启动 `open-computer-use` 的配置。
  默认 MCP 继续受受管 MCP allowlist 限制；未安装运行程序时不注册，不影响 CLI 启动。

本改造内置的是 skill 和接入规则，不包含第三方运行程序、Chrome 扩展或系统权限。
这些仍按各项目安装说明准备；fork 不自动安装、升级或授权它们。
本行为也适用于 fork 提供的 app-server；官方 Desktop 自带的程序和配置保持原样。

## 更新第三方 skill

使用精简 vendor 快照而非 submodule：只保存 skill、引用文件和许可证，
避免把两个项目的运行时源码及构建工具链带入 fork。
维护机需要 Python 3、已认证的 GitHub CLI 和网络；构建及运行不需要它们。

```sh
# 离线核对全部文件和来源清单；CI 也执行此命令。
python3 scripts/fork/sync_skills.py check

# 查询当前跟踪 ref 是否有更新，不修改文件。
python3 scripts/fork/sync_skills.py check --upstream

# 更新两个 skill 到各自 main 的当前提交。
python3 scripts/fork/sync_skills.py update

# 单独更新；也可用 --ref 指定 tag 或 commit。
python3 scripts/fork/sync_skills.py update open-browser-use --ref main
```

脚本从固定仓库解析 ref 到完整 commit，再按 commit 下载归档；
所有选定来源下载和校验成功后才替换快照，并生成
[`skills.lock.json`](../../scripts/fork/skills.lock.json)，记录仓库、ref、commit、路径及全部文件摘要。
拒绝导入符号链接或越界路径，保留仓库许可证。
更新后审查快照及 lock 差异，确认 skill 指令适用于本机 runtime，再按 fork 流程验证和发布。
运行时版本单独更新；skill 最新不代表本机 CLI、扩展及 SDK 已同步。

## 入口和验证

- [插件过滤规则](../../codex-rs/core-plugins/src/fork_policy.rs)、
  [加载入口](../../codex-rs/core-plugins/src/loader.rs)、
  [模型工具建议](../../codex-rs/core-plugins/src/discoverable.rs)、
  [远端推荐](../../codex-rs/core-plugins/src/remote.rs)。
- [skill 内置及独立缓存](../../codex-rs/skills/src/lib.rs)、
  [用户 skill 优先](../../codex-rs/ext/skills/src/loader/host_merge.rs)、
  [默认 MCP](../../codex-rs/core/src/config/fork_automation.rs)。
- [快照更新器](../../scripts/fork/sync_skills.py)及其脚本测试覆盖文件完整性、失败不替换和归档安全边界。
  Rust 定向测试覆盖本地及远端插件过滤、hooks 过滤、保留配置及缓存、skill 发现与用户覆盖，
  以及 MCP 缺失运行程序、已有别名和显式禁用项；在[自有 CI](../../.github/workflows/fork.yml)运行。

## 上游同步

过滤必须放在远端安装配置合并之后，并同时覆盖完整加载、hooks-only、模型建议与推荐路径。
上游新增官方自动化插件名称或 marketplace 时复核清单，不按任意名字子串屏蔽第三方插件。
上游变更 skill 缓存、发现或 MCP catalog 时，保留 Desktop 隔离、用户配置优先和受管约束。
新增、更新、删除快照文件都必须使嵌入资源重新构建，并改变安装缓存指纹。
