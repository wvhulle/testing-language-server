# assert-lsp

LSP server that shows test failures as diagnostics. Zero configuration for standard projects.

Supported: `cargo test`, `cargo nextest`, Jest, Vitest, Node Test Runner, `go test`, `deno test`, PHPUnit.

## Installation

```sh
cargo install assert-lsp
```

## Editor Setup

Helix (`~/.config/helix/languages.toml`):

```toml
[language-server.assert-lsp]
command = "assert-lsp"

[[language]]
name = "rust"
language-servers = [{ name = "assert-lsp", only-features = ["diagnostics"] }, "rust-analyzer"]
```

Neovim:

```lua
vim.lsp.start({ name = "assert-lsp", cmd = { "assert-lsp" }, root_dir = vim.fn.getcwd() })
```

## Configuration

Optional `.assert-lsp.toml` in project root:

```toml
[adapter_command.cargo-test]
test_kind = "cargo-test"
extra_arg = ["--workspace"]
env = {}
include = ["**/*.rs"]
exclude = ["**/target/**"]
```

Debug: `RUST_LOG=debug assert-lsp`

## License

MIT
