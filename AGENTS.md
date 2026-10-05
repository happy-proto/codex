# Fork development

This repository is the personal `happy-proto/codex` fork. Use
`.agents/skills/codex-fork-maintenance/SKILL.md` for fork changes, stacked PRs,
upstream alpha synchronization, releases, or installation.

- `origin` is `happy-proto/codex`; `upstream` is `openai/codex`.
- `main` is the unmodified selected upstream alpha release commit.
- `fork` is the complete stack and the default branch. Keep feature PRs open;
  do not merge them into `main`.
- Put changes in the layer that owns their purpose and rebase dependent layers.
- Preserve upstream licenses and notices. This fork does not add localization.
- Follow the existing Rust/Cargo/just toolchain. Keep local work lightweight:
  formatting, static checks, focused script tests, and downloaded-artifact smoke
  tests. Run full suites, compilation, and release builds in GitHub Actions;
  do not start expensive local work without explicit authorization.
- PRs and commits use English. Repository-local maintenance guidance may use
  Chinese. Do not change upstream APIs merely to simplify fork maintenance.
