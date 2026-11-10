# Stitchr for agents

To use the `stitchr` CLI from an agent, read [docs/agents.md](docs/agents.md). It covers the recommended workflow, the versioned JSON contract, error codes, exit codes, and composition rules.

- Installed interface: `stitchr --json --help-json`
- Runnable cookbook: [docs/examples/README.md](docs/examples/README.md)
- Machine-readable cookbook: [docs/examples/cases.json](docs/examples/cases.json)

To work on this repository, run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` from the root. `nix develop` provides the toolchain.
