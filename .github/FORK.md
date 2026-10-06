# Integration branch

`fork` is the default branch and the top of the long-lived PR stack. It contains
repository maintenance, alpha release/update support, and all active feature
layers. Its push workflow builds the complete product and publishes only after
verification succeeds.

New features are inserted below this integration layer. The feature branches
and their PRs retain individual maintenance boundaries; this aggregate PR is
not merged into `main`.

Use the repository-local maintenance skill for the current stack and selected
alpha baseline. Do not infer them from PR numbers or from upstream's latest
development commit.

## Planned features

- [CJK Markdown emphasis boundaries](https://github.com/openai/codex/issues/37531):
  prevent assistant-generated bold text from exposing literal `**` around CJK
  punctuation. Reproduce in the CLI and identify the generation or rendering
  layer before choosing a fix. The upstream report concerns the official App;
  client coverage must be established without assuming this fork distributes
  that App. Implement as a separate feature layer after the first OSC 7 release.
