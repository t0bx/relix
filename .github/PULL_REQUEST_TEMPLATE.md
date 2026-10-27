## What this changes

<!-- What does this pull request do and why? Link related issues (e.g. "Closes #123"). -->

## How I tested it

<!--
Describe what you ran and what you checked manually, e.g. engines and versions used,
terminal, operating system, and the workflow you exercised in the TUI.
-->

## Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --workspace --all-features` passes
- [ ] Integration tests run where a driver changed (`RELIX_INTEGRATION=1`, see [CONTRIBUTING.md](../CONTRIBUTING.md))
- [ ] New behaviour is covered by tests
- [ ] Documentation updated (README, `docs/`, `ARCHITECTURE.md`, key bindings) where needed
- [ ] New Rust files carry the `@author` / `@since` header
- [ ] Commits follow the Conventional Commits format
- [ ] No secrets, credentials or personal data in code, tests or logs
- [ ] Project and settings files stay compatible with older releases, or are migrated
- [ ] No unrelated changes snuck in
