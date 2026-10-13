# Codex 个人 fork

这是个人自用的 [Codex CLI](https://github.com/openai/codex) fork，跟进上游 alpha 版本。
官方 CLI 的一些功能需求和 Bug Issue 长期排不上期，因此在这里维护自己需要的功能和修复。

主要改造包括中文 Markdown 与主题配色、OSC 7 工作目录、列表展示、
显式 skill 调用、容量错误无限重试，以及会话名称同步。
完整说明见 [编号改造记录](docs/fork-changes/README.md)。

## 安装

支持 macOS Apple Silicon 和 Linux AMD64（glibc）。
Linux 需要 Python 3、flock、tar 和 xz；macOS 包采用 ad-hoc 签名。

```sh
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/fork/scripts/fork/install.sh | sh
```

默认安装到 `~/.local/bin`，继续使用现有配置、凭据和会话。
请将该目录加入 PATH，再运行 `codex`。

## 更新与回退

启动时只提示新版本，使用以下命令主动更新或回退：

```sh
codex update
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/fork/scripts/fork/install.sh | sh -s -- --rollback
```

## 维护

`main` 保存选定上游版本，`fork` 保存全部改造；推送 `fork` 后由 CI 验证并发布。
维护规则见 [AGENTS.md](AGENTS.md)，操作方法见 [维护 skill](.agents/skills/codex-fork-maintenance/SKILL.md)。
安装、版本和发布细节见 [分发与更新说明](docs/fork-changes/0001-distribution-and-updates.md)。

[上游介绍与文档](docs/upstream-readme.md) · [Apache-2.0 License](LICENSE)
