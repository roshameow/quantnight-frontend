# QuantNight

[中文](README.md) · [Download](https://github.com/roshameow/quantnight-frontend/releases/latest) · [User guide](docs/user-guide.en.md)

A desktop console for quantitative research tasks: manage regular, Priority and Super jobs, track local and remote execution, and explore results.

![QuantNight task manager demo](assets/demo.gif)

## Try the interface

Run an interactive demo with synthetic data. No database, platform account or private backend is required.

```bash
git clone https://github.com/roshameow/quantnight-frontend.git
cd quantnight-frontend
npm ci
npm run demo
```

Open `/demo.html` at the address printed by Vite. Browse task cards, toggle local/remote labels and filter sample results. Start, pause and delete actions change in-memory demo data only; refresh resets it. No simulations are submitted. Requires Node.js 22 or later.

## Scope

| Component | Availability |
| --- | --- |
| Vue + Tauri desktop UI | Public source in this repository |
| Task, configuration and result views | Desktop use requires configured MongoDB; the demo does not |
| Simulation execution, task generation and remote scripts | Separate private `quantnight` backend; not included |

The desktop download does not include the execution backend. The full workflow requires a compatible backend, script mappings and the appropriate platform account. Demo results are fictional and are not evidence of investment performance.

## Desktop installation and development

Download a macOS or Windows build from [Releases](https://github.com/roshameow/quantnight-frontend/releases/latest), then follow the [configuration guide](docs/user-guide.en.md).

```bash
npm ci
npm run tauri:dev
```

Building the desktop app also requires Rust and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Priority scheduling and paused monitoring

With a compatible per-root backend, **Scheduling priority** is a positive weight (default 1): larger values run first, not a stage number or simulation budget. Within one original source consumer, ordering is **task weight → R3/R2/R1 → started roots within the stage → stable order**. Running simulations are not preempted; separate consumers/hosts do not provide strict account-wide ordering. Each root retains its original source and independent limit of three stages with up to 60 candidates each.

A stop request first shows `stopping`. The card shows confirmed `paused` only after the backend verifies that the exact session and recorded execution processes have exited, validates task identity/target, and persists the receipt. Ctrl+C, success logs or an old paused record are not exit proof; failed/timed-out verification remains unconfirmed. Cached or late `active`/`blocked` research metadata cannot override the stop operation state, and a failed list refresh does not revoke verified exit evidence. Root progress, actual results and budgets are preserved. Research snapshots do not establish process liveness, and controller exit does not establish platform simulation completion.

For a paused legacy task whose old campaign summary is confirmed absent, monitoring preserves previous counts and marks progress unconfirmed instead of repeatedly reporting missing-ledger errors. Existing campaigns still use exact-ID receipt monitoring; real network/ledger errors remain visible. No task is resumed automatically.

Regular/SUPER cards also distinguish control records from current worker observations: `running` requires fresh Python/uv evidence matching the task ID and target. A retained bash/sleep shell reports no observed consumer; failed checks remain unconfirmed, not paused or platform-simulation completion.

An alive consumer need not be dispatching: fresh, task/target-matched metadata distinguishes request-outcome reconciliation from waiting for quota recovery. Both retain the process, heartbeat and accepted-result collection; neither proves task exit or a confirmed pause. Missing, stale or conflicting metadata and a missing worker cannot be presented as a healthy waiting consumer.

These changes require updated backend consumers and a rebuilt native frontend. Editing source or refreshing a web page does not upgrade running processes or publish new installers.

## Documentation

- [User guide](docs/user-guide.en.md): setup, tasks and results.
- [Architecture](docs/architecture.en.md): database routing, local/remote semantics and script execution.
- [Changelog](CHANGELOG.md): source and installer version notes.
- [Report an issue](https://github.com/roshameow/quantnight-frontend/issues): include your version, OS, reproduction steps and redacted logs.

## Version and license

Source version: **1.2.4**. Available binaries are listed in Releases. Historical `v1.2.3` assets used `0.1.0` filenames; see the changelog.

[MIT](LICENSE)

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
