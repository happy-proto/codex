# Codex 个人 fork

这是 [happy-proto/codex](https://github.com/happy-proto/codex)，基于
[OpenAI Codex](https://github.com/openai/codex) 的个人 fork。跟进上游 **alpha**
版本，用长期 PR stack 维护自定义功能。公开提供可复现的安装方式，暂不承诺大众支持。

- `main`：选定上游 alpha Release 的原始提交。
- `fork`：完整功能 stack 的顶部，也是默认分支。
- 功能 PR 长期开启，升级上游 alpha 时重放各层修改。
- 不新增 i18n，也不分发官方 Desktop 应用。

## Install this fork

Only macOS Apple Silicon is supported initially. The complete package includes
the code-mode host, ripgrep, and patched zsh. It is ad-hoc signed, not Apple
notarized; macOS may require explicit approval on first use.

```sh
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/fork/scripts/fork/install.sh | sh
codex --version
```

The installer selects a published fork alpha Release, verifies the package
SHA-256, preserves the previous package, and replaces `codex` in
`~/.local/bin`. `CODEX_HOME` and `CODEX_INSTALL_DIR` override these paths.
Configuration, credentials, and sessions continue using the existing Codex home.
Add `~/.local/bin` to PATH if needed; remove a competing npm/Homebrew installation
only if you want to stop managing that installation separately.

```sh
codex update            # update explicitly; background upgrades are disabled
# Select the previous locally installed package:
curl -fsSL https://raw.githubusercontent.com/happy-proto/codex/fork/scripts/fork/install.sh | sh -s -- --rollback
```

Versions use the upstream alpha plus `.fork`, for example
`0.162.0-alpha.14.fork`. Each verified `fork` update automatically publishes.
Within the same alpha, the same Release and tag are updated; version strings
alone do **not** identify a build. `fork-release.json` records source and upstream
commits and the package digest. Package asset names include their digest and
older packages remain available. Fork updates never select the official
distribution; upstream alpha synchronization is initiated by the maintainer.

The first switch from an official standalone package also preserves it under
`~/.codex/packages/standalone/official-before-fork`. You can return to the
official distribution by rerunning its installer below with the desired version.

## Maintenance

The repository-local [maintenance skill](.agents/skills/codex-fork-maintenance/SKILL.md)
defines change ownership, alpha synchronization, and stack verification.
Keep local work lightweight: compilation and heavy tests run in GitHub Actions.
The [fork workflow](.github/workflows/fork.yml) tests installation contracts,
builds and signs the complete package, tests update behavior, and smoke-tests
the actual CLI and app-server before publishing. Upstream workflows that require
OpenAI infrastructure are disabled in this fork's GitHub settings.
PRs run lightweight script checks; `fork` pushes run the complete build and Rust
tests once for the integrated stack.
CI keeps release optimization and disables LTO for the standard macOS runner.

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
