# Native taskctl — shared core, not automatically installed

All 16 original commands call `task-core::task_control::TaskControl`, also used
by the desktop. The CLI does not implement task/DB/receipt policy, link Tauri,
start a daemon, or fall back to the retired Python controller.

## Build and select configuration

```sh
cargo build --offline --locked --manifest-path crates/Cargo.toml -p quantnight-taskctl --bin taskctl
crates/target/debug/taskctl --help
```

Configuration selectors are explicit prefix options, mutually exclusive:

```text
taskctl --backend-root /absolute/backend/root COMMAND [original arguments]
taskctl --app-config /explicit/existing/config.toml COMMAND [original arguments]
```

`--backend-root` asks the ORIGINAL backend MongoConfig owner for a minimal
AppConfig through a private subprocess pipe. No new YAML parser, temporary
credential file, config search or connection fallback is added. Exported private
endpoints are neither logged nor included in failures. The shared core validates
the binding, then lazily constructs only the requested configured role. Missing
remote is allowed for local-only operation; remote access still fails explicitly.
Canonical result namespaces use the original profile mapping when requested.

`--app-config` retains the original shared AppConfig loader. Relative explicit
paths resolve from the caller's cwd. `start --config JSON` remains the business
configuration, not a file path. Help and syntax errors occur before configuration
or clients: exit0 for help, exit2 for syntax. Business stdout is one versioned
JSON result with the core's exit code, including Priority status ok=false/exit0.
Diagnostics use stderr.

## Old Python path

The backend's `.agents/skills/manage-quantnight-frontend/scripts/taskctl.py` is
now stdlib-only exec forwarding. It selects exactly:

1. explicit absolute `QUANTNIGHT_TASKCTL_BIN`, when set; otherwise
2. `<backend>/bin/taskctl`.

It injects its own `--backend-root`, preserves original business argv and process
streams/cwd/exit, and never invokes Cargo, searches another checkout/PATH or
loads a legacy controller. Missing/non-executable/self-referential binary fails
closed. Installation or PATH changes are separate authorized actions; this
source migration does not install a binary. The running desktop/CLI adoption
must be verified separately.

The retired controller exists only under backend `tests/reference/` as an
isolated, disabled-by-default oracle. Reference-only green is not acceptance.
Professional Python generation/research/recovery/notification owners remain in
use; native task control does not mean all quantitative code is Python-free.

## Acceptance

The original backend gate requires an explicit matching Rust workspace:

```sh
QUANTNIGHT_NATIVE_MANIFEST=/explicit/frontend/crates/Cargo.toml \
  bash scripts/check_architecture.sh
```

It runs existing Python contracts plus supplemental reference evidence and the
actual Rust core (`test-support`) / CLI (`backend-owner-tests`) suites. The CLI
feature tests execute the original Python config owner in a synthetic backend
and the actual compatibility path with this built binary; network/clients are
forbidden. Paired committed acceptance also requires `--native-ref` and archives
both source revisions, never an unrelated dirty Rust tree.

Standalone CLI-only tests:

```sh
cargo test --offline --locked --manifest-path crates/Cargo.toml -p quantnight-taskctl
```

Parser integers are retained without truncation; converting impossible native
representations (including negative generate timeout) fails explicitly before
config/DB, never wraps or clamps. Normal operation limits/defaults stay with the
core. Sync's historical unused timeout and delete's force remain parsed and do
not create new deadlines or bypass safety. Help startup measurements do not
measure DB/SSH/GUI operation latency.
