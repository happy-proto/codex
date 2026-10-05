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
