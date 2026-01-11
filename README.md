# testing-language-server

⚠️ **IMPORTANT NOTICE**
This project is under active development and may introduce breaking changes. If you encounter any issues, please make sure to update to the latest version before reporting bugs.

General purpose LSP server that integrate with testing.
The language server is characterized by portability and extensibility.

## Motivation

This LSP server is heavily influenced by the following tools

- [neotest](https://github.com/nvim-neotest/neotest)
- [Wallaby.js](https://wallabyjs.com)

These tools are very useful and powerful. However, they depend on the execution environment, such as VSCode and Neovim, and the portability aspect was inconvenient for me.
So, I designed this testing-language-server and its dedicated adapters for each test tool to be the middle layer to the parts that depend on each editor.

This design makes it easy to view diagnostics from tests in any editor. Environment-dependent features like neotest and VSCode's built-in testing tools can also be achieved with minimal code using testing-language-server.

## Instllation

```sh
cargo install testing-language-server
cargo install testing-ls-adapter
```

## Features

- [x] Realtime testing diagnostics
- [x] [VSCode extension](https://github.com/kbwo/vscode-testing-ls)
- [x] [coc.nvim extension](https://github.com/kbwo/coc-testing-ls)
- [x] For Neovim builtin LSP, see [testing-ls.nvim](https://github.com/kbwo/testing-ls.nvim)
- [ ] More efficient checking of diagnostics
- [ ] Useful commands in each extension

## Configuration

### Required settings for all editors
You need to prepare .testingls.toml. See [this](./demo/.testingls.toml) for an example of the configuration.

```.testingls.toml
enableWorkspaceDiagnostics = true

[adapterCommand.cargo-test]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=cargo-test"]
include = ["/**/src/**/*.rs"]
exclude = ["/**/target/**"]

[adapterCommand.cargo-nextest]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=cargo-nextest"]
include = ["/**/src/**/*.rs"]
exclude = ["/**/target/**"]

[adapterCommand.jest]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=jest"]
include = ["/jest/*.js"]
exclude = ["/jest/**/node_modules/**/*"]

[adapterCommand.vitest]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=vitest"]
include = ["/vitest/*.test.ts", "/vitest/config/**/*.test.ts"]
exclude = ["/vitest/**/node_modules/**/*"]

[adapterCommand.deno]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=deno"]
include = ["/deno/*.ts"]
exclude = []

[adapterCommand.go]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=go-test"]
include = ["/**/*.go"]
exclude = []

[adapterCommand.node-test]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=node-test"]
include = ["/node-test/*.test.js"]
exclude = []

[adapterCommand.phpunit]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=phpunit"]
include = ["/**/*Test.php"]
exclude = ["/phpunit/vendor/**/*.php"]
```

### VSCode

Install from [VSCode Marketplace](https://marketplace.visualstudio.com/items?itemName=kbwo.testing-language-server).
You can see the example in [settings.json](./demo/.vscode/settings.json).

### coc.nvim
Install from `:CocInstall coc-testing-ls`.
You can see the example in [See more example](./.vim/coc-settings.json)

### Neovim (nvim-lspconfig)

See [testing-ls.nvim](https://github.com/kbwo/testing-ls.nvim)

### Helix

Add to your `~/.config/helix/languages.toml`:

```toml
[language-server.testing-ls]
command = "testing-language-server"

# Optional: pass config via initializationOptions instead of .testingls.toml
[language-server.testing-ls.config]
enableWorkspaceDiagnostics = true

[language-server.testing-ls.config.adapterCommand.cargo-test]
path = "testing-ls-adapter"
extra_arg = ["--test-kind=cargo-test"]
include = ["/**/*.rs"]
exclude = ["/**/target/**"]

[[language]]
name = "rust"
language-servers = [
  { name = "testing-ls", only-features = ["diagnostics"] },
  "rust-analyzer"
]
```

**Note:** Use `only-features = ["diagnostics"]` to prevent testing-ls from interfering with other LSP features provided by rust-analyzer.

See [demo/.helix/languages.toml](./demo/.helix/languages.toml) for more examples.

## Adapter
- [x] `cargo test`
- [x] `cargo nextest`
- [x] `jest`
- [x] `deno test`
- [x] `go test`
- [x] `phpunit`
- [x] `vitest`
- [x] `node --test` (Node Test Runner)

### Writing custom adapter
⚠ The specification of adapter CLI is not stabilized yet.

See [ADAPTER_SPEC.md](./doc/ADAPTER_SPEC.md) and [spec.rs](./src/spec.rs).

## Configuration Reference

### `InitializedOptions`

Configuration can be provided via `.testingls.toml` in the project root, or via LSP `initializationOptions`.

| Field | Type | Description |
|-------|------|-------------|
| `enableWorkspaceDiagnostics` | `boolean` | Enable diagnostics for all files in workspace |
| `adapterCommand` | `object` | Map of adapter configurations (see below) |

### `AdapterConfiguration`

Each adapter is configured under `adapterCommand.<name>`:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `path` | `string` | Yes | Path to adapter binary (absolute or in PATH) |
| `extra_arg` | `string[]` | No | Extra arguments passed after `--` to adapter |
| `env` | `object` | No | Environment variables for adapter process |
| `include` | `string[]` | Yes | Glob patterns for files to include |
| `exclude` | `string[]` | Yes | Glob patterns for files to exclude |
| `workspace_dir` | `string` | No | Override workspace directory |

## Troubleshooting

### "Adapter failed with exit code X"

This error occurs when the adapter process exits with a non-zero status.

**Common causes:**
- Test framework not installed (e.g., `cargo`, `npm`, `go` not in PATH)
- Invalid `path` in adapter configuration
- Missing dependencies in the project

**Solutions:**
1. Verify the adapter binary exists: `which testing-ls-adapter`
2. Run the adapter manually to see detailed errors:
   ```sh
   testing-ls-adapter discover --file-paths src/main.rs -- --test-kind=cargo-test
   ```
3. Check if the test framework works: `cargo test` / `npm test` / etc.

### "Adapter produced no output"

The adapter ran but didn't return any JSON output.

**Common causes:**
- Wrong `include` patterns - no files matched
- Adapter crashed before producing output
- Wrong `--test-kind` argument

**Solutions:**
1. Verify include patterns match your files
2. Check adapter logs (if enabled)
3. Run adapter manually with `--file-paths` pointing to a test file

### "Failed to parse adapter output"

The adapter produced output but it wasn't valid JSON.

**Common causes:**
- Adapter version mismatch
- Adapter writing debug output to stdout

**Solutions:**
1. Update both `testing-language-server` and `testing-ls-adapter` to the same version
2. Check for print statements or debug output in custom adapters

### Debugging

Enable debug logging by setting the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug hx .
```

Adapter logs are written to the working directory as `<adapter>_test.log` (e.g., `cargo_test.log`).