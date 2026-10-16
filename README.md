# Codex 个人 fork

这是个人自用的 [Codex CLI](https://github.com/openai/codex) fork，跟进上游 alpha 版本。
官方 CLI 的一些功能需求和 Bug Issue 长期排不上期，因此在这里维护自己需要的功能和修复。

主要改造包括中文 Markdown、OSC 7 工作目录、列表展示、
显式 skill 调用、容量错误无限重试，以及会话名称同步。
完整说明见 [编号改造记录](docs/fork-changes/README.md)。

浏览器与电脑操作默认使用 [Open Browser Use 和 Open Computer Use](docs/fork-changes/0010-open-automation.md)。
内置对应 skills，使用本机已安装的运行程序；仅在 fork 内过滤官方自动化插件，
不修改与官方 Desktop 共用的插件配置。

## 安装

支持 macOS Apple Silicon 和 Linux AMD64（glibc）。
Linux 需要 Python 3、flock、tar 和 xz；macOS 包采用 ad-hoc 签名。

```sh
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/main/scripts/fork/install.sh | sh
```

默认安装到 `~/.local/bin`，继续使用现有配置、凭据和会话。
请将该目录加入 PATH，再运行 `codex`。

## 更新与回退

启动时只提示新版本，使用以下命令主动更新或回退：

```sh
codex update
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/main/scripts/fork/install.sh | sh -s -- --rollback
```

默认分支迁移到 `main` 后，仍内置旧分支安装链接的版本需要先运行一次上面的安装命令；
新版本继续使用 `codex update`，已有配置和会话不变。

## 维护

`main` 是唯一长期分支，保存全部改造；定期 rebase 到已发布的上游 alpha，推送后由 CI 验证并发布。
维护规则见 [AGENTS.md](AGENTS.md)，操作方法见 [维护 skill](.agents/skills/codex-fork-maintenance/SKILL.md)。
安装、版本和发布细节见 [分发与更新说明](docs/fork-changes/0001-distribution-and-updates.md)。

[上游介绍与文档](docs/upstream-readme.md) · [Apache-2.0 License](LICENSE)
