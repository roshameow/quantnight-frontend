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
now stdlib-only exec forwarding. Selection is explicit and durable:

1. Registered dev mode selects the one canonical GUI host and inserts `taskctl`.
2. Absolute `QUANTNIGHT_TASKCTL_BIN` remains explicit standalone intent only when
   it does not conflict with dev registration/environment.
3. A durable standalone registration permits `<backend>/bin/taskctl`.

Missing/invalid enrollment without explicit standalone intent is an error, not
implicit release authority. See the setup contract below; this changes the old
unenrolled default-bin behavior deliberately.

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

## Explicit desktop development taskctl mode

The standalone `crates/taskctl` executable and task core remain Tauri/linfa-free.
An explicitly dev-selected **GUI executable** runs the very same CLI library
entry before GUI configuration, Mongo, Tauri initialization or either window loop.
Its OS loader still loads desktop frameworks; it is not a server/headless-runtime
replacement and has a separately measured startup cost.

Main/installer explicitly binds canonical frontend/backend/executable paths in a
non-secret JSON file (`schema_version: 1`, `frontend_root`, `backend_root`,
`executable`, `target`, `profile: "debug"`). The backend root comes from the original
GUI AppConfig `env.working_dir`, not checkout guessing or a second TOML parser.
The executable and binding file's canonical parents must already exist. One-time
explicit enrollment writes fixed `.quantnight/taskctl-selection.json` locators at
both known roots, referring to ONE canonical non-secret binding document. After
that, unchanged `npm run tauri dev` and the original backend shim work in fresh
independent/Pi-like environments without selector variables. Vite/build/runtime
and shim verify the registered pair. If `QUANTNIGHT_DEV_BINDING` is supplied, it
must agree; it cannot override missing/conflicting enrollment.

For Main's separately authorized installation, first review the plan (no writes):

```sh
node scripts/register-dev-host.mjs --mode dev \
  --frontend-root /canonical/frontend --backend-root /canonical/backend \
  --executable /canonical/target/debug/quantnight-frontend \
  --binding /canonical/existing-parent/dev-binding.json --target aarch64-apple-darwin
# Only after explicit setup authorization, add --write to that same command.
```

Setup accepts explicit backend root rather than parsing private TOML; Main must
verify it against the original AppConfig owner. GUI startup still enforces that
match before clients. Existing conflicting enrollment is never overwritten by an
environment override or automatic hook. Tests enroll only disposable/snapshot
fixtures; no actual checkout is enrolled or adopted by this change.

**Compatibility change:** absence of shim enrollment no longer authorizes implicit
`ROOT/bin/taskctl` release execution. An explicit `standalone` locator permits that
project binary (`--mode standalone --backend-root /canonical/backend --write`), or
use the existing absolute `QUANTNIGHT_TASKCTL_BIN` when no dev registration conflicts.
Direct standalone binaries remain untouched. Missing/broken dev locators/binding
and conflicting standalone/dev selections fail clearly, never fallback.

Vite extends its existing watcher for core/CLI sources, shared identity owner,
build scripts and manifests/locks (including the exact GUI Cargo.lock); its stamp inside Tauri's watched `src/` causes
stock Tauri 2.4.1 kill/wait/build/restart. Neither shim nor build.rs invokes Cargo,
and no second watcher, runner or rebuild broker is installed. Core and CLI each
export their own build-step source identity, compared by the host using the same
Rust fingerprint implementation. macOS mapped-image UUID, not a current_exe file
hash, distinguishes the loaded host from a replacement on disk. Native process
start identity and an OS-held GUI lock fence stale/live ready records.

Freshness is checked **at admission**. Already-admitted work is not atomically
cut over; Python professional helpers remain original fresh owners with explicit
backend roots. No Python/tmux/MCP/remote consumer is restarted. These checks do
not promise an immutable compiler snapshot or protection against arbitrary
concurrent rewrites/restored metadata/ABA.

Normal successful edits and compile-error recovery follow stock Tauri behavior.
Startup/runtime admission rejection may terminate that dev lifecycle: **fix the
reported cause and rerun `npm run tauri dev`**. A missed initial watch event is a
fail-closed diagnostic, not a promise of automatic recovery. No parked unready
process or spoofed compiler error is used. If the GUI is closed and no conflicting
live owner exists, a fresh last-built dev generation may serve dev CLI operations;
standalone operations never require an open GUI. Binding/adoption remains an
explicit operator action, not a test-side installation.
