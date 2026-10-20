# 会话动态补全

## 动机与行为

`codex completion` 生成的静态脚本不能读取当前会话。启用 `clap_complete` 的
`unstable-dynamic`，为 `codex resume` 和 `codex fork` 的第一个位置参数提供动态候选。

- 默认列出当前目录的最近活动会话，`--all` 取消目录筛选；支持根命令与子命令的 `-C`。
- UUID 和名称均支持任意位置的连续子串匹配，ASCII 字母忽略大小写；
  可直接输入 UUID 中段或尾段，不要求从 UUIDv7 的时间前缀开始输入。
  最多返回 100 条，按近期活动排序，不做跳字模糊匹配。
- 命中 UUID 时只展示并填入完整 UUID；命中名称时只展示并填入完整名称，
  不附加目录、UUID、预览或重复说明。同一会话两个字段都命中时 UUID 优先。
  空查询只展示最近会话的 UUID。
- 名称候选必须能唯一解析到一个交互会话。重名、控制字符、以 `-` 开头或可被解析为 UUID 的名称、
  未命名会话的预览和非交互会话名称不作为候选；这些会话仍可按 UUID 查找。
  唯一性检查同时考虑其它会话展示的名称与预览，避免填入后产生歧义。
- Zsh 注册禁用对后端候选的二次前缀过滤，并保留近期活动顺序；
  原生菜单对纯 ASCII 候选的匹配段加粗，不把颜色码混入填入值。
  Zsh 5.9 在宽字符后的捕获组着色位置不准确，含中文等非 ASCII 字符的候选沿用原生样式；
  查询含 `:` 或 `=` 时也沿用原生样式，避免与颜色规则分隔符冲突。
  Fish、Bash、PowerShell 保留 shell 自身的展示样式；
  当前 Fish 补全接口没有独立的匹配段样式字段，不强行插入 ANSI 控制字符。
- `--last` 后的位置是 prompt，不提供会话候选；已有会话参数后的 prompt 也不提供候选。
- `resume --include-non-interactive` 额外包含 exec 和 app-server 会话；默认排除归档、
  非交互与子 agent 会话。显式 `--remote` 不读取本地候选，包括光标后的选项。
- 附属于父会话的 attached thread 不作为独立补全候选，也不参与名称冲突判断，
  与上游普通会话列表的可见性保持一致；仍可通过父会话使用相应接口访问。

## 启用

新二进制安装后，在对应 shell 中加载；原来的静态补全脚本不会自动变成动态补全，
应移除旧注册，避免重复候选。以下配置在 shell 启动时生成匹配当前二进制的注册脚本。

Fish（保存到 `~/.config/fish/completions/codex.fish`）：

```fish
COMPLETE=fish command codex | source
```

`codex completion fish` 默认输出上述一行动态入口，已有调用该命令的补全更新流程无需修改。
该入口同时支持普通选项、子命令与枚举值补全，包括 `--worktree`，不限于会话候选。

Zsh（在 `.zshrc` 的 `compinit` 之后）：

```zsh
source <(COMPLETE=zsh codex)
```

Bash（在 `.bashrc`）：

```bash
source <(COMPLETE=bash codex)
```

其它 shell 的 `codex completion <shell>` 仍生成静态脚本；也可按上述方式使用
clap 官方 `COMPLETE` 协议注册动态补全。

## 实现边界

- [CLI 入口](../../codex-rs/cli/src/lib.rs) 在 arg0 分发和 Tokio 启动前处理补全，
  不创建辅助命令目录、不启动 daemon 或正常会话。
- [补全模块](../../codex-rs/cli/src/completion.rs) 用现有 clap 参数定义解析上下文，
  查询前只读用户 `config.toml`、显式 profile 文件和 CLI `sqlite_home` 覆盖；
  配置文件优先于 `CODEX_SQLITE_HOME`，默认使用 `CODEX_HOME`。
  不加载完整配置栈；项目配置与受管配置对存储目录的覆盖不在首版范围内。
- [状态查询](../../codex-rs/state/src/completion.rs) 通过已有 SQLite 只读入口访问现存数据库，
  不创建、迁移、修复数据库，不扫描 JSONL、名称兼容索引或压缩历史。
  只存在于旧名称索引中的名称暂不参与匹配，仍可用数据库中的 UUID 补全。
- 数据库连接与查询整体限制为 250 ms，锁等待为 20 ms；数据库缺失、旧 schema、
  配置不可读或查询失败时静默返回空候选，不影响正常 CLI 行为。
  上述超时不包含读取用户配置文件的同步文件系统耗时。
- 候选不携带说明；名称值包含控制字符时不返回该名称。
  名称唯一性在所有未归档会话中检查，避免目录筛选掩盖同名会话。
- Fish 注册脚本让 `string unescape` 解码当前 token，支持未闭合引号和转义空格；
  其余注册与输出协议沿用 clap。此处替换上游脚本的当前 token 表达式，
  上游表达式变化时明确报错，避免升级后静默丢失引号支持。
- Zsh 注册基于锁定的 clap 脚本替换 `_describe` 调用并注入菜单匹配样式；
  对普通选项与路径仍使用 clap 已筛选的候选。上游脚本变化时明确报错，
  升级 clap 时需同时验证 `_describe -U`、末尾为 `/` 的名称和原生菜单颜色。
- Bash 注册启用 Readline 原生转义，确保空格、引号和 shell 特殊字符仍属于一个参数；
  与当前 shell 目录下真实目录同名的标题不作为候选，避免 Readline 自动追加 `/` 改变目标。
  shell 类型在进入 `CompleteEnv` 前捕获；clap 会在调用命令工厂前移除 `COMPLETE`，
  工厂内不能再读取该变量判断是否启用 Bash 目录标题过滤。
  PowerShell 的填入值使用单引号转义，菜单仍显示原始值；不把转义后的值写入状态数据库。

## 验证与维护

[单元测试](../../codex-rs/cli/src/completion_tests.rs) 覆盖参数上下文、光标后的远端选项、
UUID 中段/尾段与唯一名称子串填入、字段优先级、精简展示、重名与控制字符、配置路径解析。
[集成测试](../../codex-rs/cli/tests/completion.rs) 使用临时最小数据库和真实 CLI 补全协议，
覆盖来源/目录/归档筛选、中文名称查询、Fish/Zsh 返回值、`fork`、空数据库与 `--last`，
并比较数据库内容，确认读取未触发迁移或写入。两组测试接入 fork CI。
另有 [SQL 回归](../../scripts/fork/test_session_completion.py) 直接执行生产查询，
覆盖 UUIDv7 子串与大小写、标题唯一性、预览冲突、来源/目录筛选和结果上限；无需本地编译。
集成测试还使用实际 Fish 加载 `codex completion fish` 生成的入口，验证中文、
未闭合单双引号、转义空格、`--worktree`、普通子命令和枚举值；
CI 安装 Fish，本地缺少 Fish 时跳过这一项，也可用 `CODEX_FISH_TEST_BINARY` 指定可执行文件。

真实交互验收通过伪终端发送 Tab，确认名称和 UUID 子串能填入有效参数；
Fish 与 Bash 额外覆盖默认目录筛选、未闭合引号和转义空格。
名称匹配后填入唯一名称，由 shell 处理空格和引号；ID 匹配后填入完整 UUID。
Zsh 需同时验证 UUID 尾段、名称中段、大小写、末尾 `/` 的名称和纯 ASCII 匹配段加粗，
确认中文候选不会错误套用 ASCII 匹配样式。

上游同步时关注 CLI 参数位置、会话来源映射、展示名称语义和 SQLite schema。
挂接补全器时使用保持参数顺序的 `mut_args`，避免 `mut_arg` 的移除/追加行为
改变隐式位置参数索引；保留首参数为会话、第二参数为 prompt 的回归检查。
clap 动态 API 与 shell 协议不稳定；升级时应同时验证注册脚本、引号、中文和空格名称，
以及真实 shell 的二次过滤行为，不能只检查 Rust 候选函数。
