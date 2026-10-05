# Codex 个人 fork

这是 [happy-proto/codex](https://github.com/happy-proto/codex)，基于
[OpenAI Codex](https://github.com/openai/codex) 的个人 fork。跟进上游 **alpha**
版本，用长期 PR stack 维护自定义功能。公开提供可复现的安装方式，暂不承诺大众支持。

- `main`：选定上游 alpha Release 的原始提交。
- `fork`：完整功能 stack 的顶部，也是默认分支。
- 功能 PR 长期开启，升级上游 alpha 时重放各层修改。
- 不新增 i18n，也不分发官方 Desktop 应用。

## 安装此 fork

首期仅支持 macOS Apple Silicon。完整包包含 code-mode host 和 ripgrep；从上游
`0.163.0-alpha.1` 起不再附带补丁 zsh，执行命令沿用上游的系统 shell 路径。
采用 ad-hoc 签名，未经 Apple 公证；macOS 首次运行时可能需要手动批准。

```sh
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/fork/scripts/fork/install.sh | sh
codex --version
```

安装器选择已发布的 fork alpha Release，验证安装包 SHA-256，保留上一个安装包，
并替换 `~/.local/bin` 中的 `codex`。可用 `CODEX_HOME` 和 `CODEX_INSTALL_DIR` 指定路径。
安装包下载检测到 `axel` 时使用其默认并行度，失败后清理未完成的文件并回退到 curl；
未安装 axel 时直接使用 curl。元数据始终用 curl 获取，安装前仍校验整个包的 SHA-256。
继续使用现有 Codex 目录中的配置、凭据和会话。按需将 `~/.local/bin` 加入 PATH；
如果同时安装了 npm/Homebrew 版本，可以保留独立管理，或在确定不用后移除。

```sh
codex update            # 手动更新，默认关闭后台升级
# 回退到此前安装的包：
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/fork/scripts/fork/install.sh | sh -s -- --rollback
```

启动后在后台异步检查 fork Release，优先使用 `GH_TOKEN`、`GITHUB_TOKEN` 或已登录的 `gh`。
发现新构建只显示一次 warning，不等待网络、不弹更新选择框，也不自动安装；使用 `codex update`
主动更新。设置 `check_for_update_on_startup = false` 可同时关闭后台检查与提示。

版本沿用上游 alpha 并添加 `.fork`，例如 `0.162.0-alpha.18.fork`。
每次 `fork` 更新验证通过后自动发布；同一 alpha 下更新同名 Release 和标签，
因此版本字符串不能唯一标识构建。`fork-release.json` 记录源提交、上游提交和包摘要，
安装包文件名包含摘要，旧包保持可下载。Fork 的更新渠道只选择 fork 发行版，
上游 alpha 同步由维护者发起。

安装器查询 GitHub API 时优先使用已安装且登录 GitHub.com 的 `gh` 凭据，
认证仅用于 API 查询；未安装或未登录 `gh` 时使用匿名请求，可能受出口 IP 的限流影响。

首次从官方独立安装包切换时，还会将其保留在
`~/.codex/packages/standalone/official-before-fork`。
需要返回官方版本时，可重新运行下方官方安装器并指定所需版本。

## Fork 功能

- **终端工作目录（OSC 7）**：在启动、会话切换、恢复和 worktree 切换后报告当前本地会话目录，
  让兼容终端在该目录打开新标签页、面板，并解析相对文件链接。
  跳过远程工作区和非终端输出，跟踪 [上游 issue #38229](https://github.com/openai/codex/issues/38229)。

## 维护

通用规则见 [AGENTS.md](AGENTS.md)，具体操作见仓库内的
[维护 skill](.agents/skills/codex-fork-maintenance/SKILL.md)。
本地只做轻量验证，编译和重型测试交给 GitHub Actions。
[Fork 工作流](.github/workflows/fork.yml) 验证安装契约、构建并签名完整包、
测试更新行为，并在发布前验收实际 CLI 和 app-server。
依赖 OpenAI 基础设施的上游工作流已在此 fork 的 GitHub 设置中禁用。
[PR 工作流](.github/workflows/fork-pr.yml) 只在 Linux 上运行轻量脚本检查，
不创建构建、Rust 测试和发布 Job；同一 PR 更新后取消旧检查。
推送 `fork` 后由 Fork 工作流对完整集成 stack 执行一次构建和 Rust 测试。
通用 Rust 测试在 Linux x64 上运行；只有完整 Apple Silicon 包的构建、签名、
macOS 安装契约和真实 CLI 验收使用 ARM macOS runner。
标准 macOS runner 上保留产物的 release 优化，关闭跨 crate Thin LTO。
Rust 测试与构建并行，保留 release 条件编译及关闭 debug assertions，禁用测试优化以控制成本。
Rust 编译通过 mbx 的 objects 模式缓存本次构建使用的对象，Cargo 下载目录单独缓存，
不重复归档整个 target。构建和测试分别维护缓存，手动完整实验可预热但默认不发布。
CI 上传 mbx 缓存统计和 Cargo 耗时报告，分别观察产物和测试构建；构建失败时也尽量保留报告。

下方保留的上游介绍与安装说明指向 **官方版本**，不用于安装此 fork。

---

<p align="center"><strong>Codex CLI</strong> is a coding agent from OpenAI that runs locally on your computer.
<p align="center">
  <img src="https://github.com/openai/codex/blob/main/.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
If you want Codex in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/codex/ide">install in your IDE.</a>
</br>If you want the desktop app experience, run <code>codex app</code> or visit <a href="https://chatgpt.com/codex?app-landing-page=true">the Codex App page</a>.
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Codex Web</strong>, go to <a href="https://chatgpt.com/codex">chatgpt.com/codex</a>.</p>

---

## Quickstart

### Installing and running Codex CLI

Run the following on Mac or Linux to install Codex CLI:

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | sh
```

Run the following on Windows to install Codex CLI:

```shell
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

The standalone installers download from `https://releases.openai.com/codex` by default and fall back to GitHub Releases if a metadata or asset download is unavailable. To force GitHub Releases, set `CODEX_INSTALLER_USE_RELEASES_OPENAI_COM` to `false` (`0` and `no` are also accepted):

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_INSTALLER_USE_RELEASES_OPENAI_COM=false sh
```

```powershell
$env:CODEX_INSTALLER_USE_RELEASES_OPENAI_COM='false'; irm https://chatgpt.com/codex/install.ps1 | iex
```

Codex CLI can also be installed via the following package managers:

```shell
# Install using npm
npm install -g @openai/codex
```

```shell
# Install using Homebrew
brew install --cask codex
```

Then simply run `codex` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/openai/codex/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `codex-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `codex-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `codex-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `codex-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `codex-x86_64-unknown-linux-musl`), so you likely want to rename it to `codex` after extracting it.

</details>

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Business, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
