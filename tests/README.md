# Frontend task progress regression checks

Run locally, without Tauri, databases, or servers:

```sh
node --test tests/taskStore.test.mjs
node node_modules/vite/bin/vite.js build --outDir tmp/task-progress-build
```

Verified: 39 Node tests pass using the real Vue/Pinia imports. Production Vite build succeeds; existing dependency warnings concern `@iarna/toml` browser externalization/eval and bundle size. No package changes are necessary.

## Verified methods and reproducible pitfalls

- Publish tasks and reconciled progress through one reactive snapshot. A synchronous Vue watcher test verifies that source changes never expose a mismatched task/progress pair. Register the backing ref with Pinia so state subscriptions/devtools also work.
- Cache reuse requires both task identity (`_id`, including `$oid`) and source. A different-ID, same-name task must not inherit completed counts. Source changes clear counts but retain the ordering watermark; deletion removes the cache.
- New success/error payloads carry `task_id`. Reject a supplied identity that does not match the current task, including old-task responses whose version is newer than the watermark. Missing identity remains legacy-compatible; null identity is accepted only for failures where metadata lookup could not resolve it. Such failures preserve counts and may mark the current task stale.
- Per-task failures and discovery failures preserve all six counters and set `stale`/`lastError`. `markProgressStale` accepts `task-progress-refresh-error` with scope `all` or `remote`; remote scope uses each task's current source. All three event types share each task's version watermark, so old discovery errors cannot overwrite newer successful statistics.
- Deferred promises reproducibly test out-of-order fetches and listener installation/unmount races. All three TaskManager-owned subscriptions must resolve before `frontend_ready`; cleanup covers late installation, startup failure, unmount during ready/fetch, and the runtime interval.
- Fetch ordering alone is insufficient for source mutations: an older list request can still resolve while the update command is pending. `fetch.invalidate()` invalidates all existing requests. `mutateTaskSource` invalidates before the command and again at completion, synchronously before committing the new source. Tests verify old reads completing during the mutation and reads started during the mutation completing after its commit cannot overwrite the source. Command failure also invalidates pending reads and propagates without committing.
- Fake timers avoid real five-minute waits. Scheduling observes accepted store state rather than blindly reacting to payloads. Discovery/task errors cancel completion timers without resetting counts; a separate in-flight guard prevents failure/recovery from duplicating a pending pause command.
- Sync commands must await sync, then remote-source update, then exactly one task fetch. Successful task fetch invokes `refresh_task_progress` without awaiting its background work. Failures in either sync command propagate and skip refresh.

## Contract boundaries / checkpoint

Frontend follow-up implementation and local verification are complete. It assumes the backend provides `refresh_task_progress`, the three event types, task IDs, and startup/periodic watcher behavior; live Tauri integration was not exercised.

`updated_at` is now a backend-issued strictly monotonic logical version based on Unix milliseconds, assigned at query start. Treat it as an ordering token, not wall-clock elapsed time. Strict monotonicity removes the former same-millisecond ordering ambiguity; frontend duplicate delivery of the same payload remains harmless.

New identity-bearing responses cannot contaminate a same-name recreated task. Legacy payloads without identity cannot establish this guarantee, so their compatibility is intentionally weaker. The payload still has no source generation, so an old same-source event after a local→remote→local round-trip is distinguishable only by its version and the retained watermark.

`src/main.js` global listener integration and both TaskCards are owned by the main worker and are not edited here. TaskManager registers and cleans up its own success, task-error and discovery-error subscriptions, runtime interval and auto-pause timers.

## Backend progress regression checks

```sh
cd src-tauri
cargo check
cargo test --bin quantnight-frontend watcher::tests -- --nocapture
# Optional, read-only: requires config.toml and the user's local/remote MongoDBs.
cargo test --bin quantnight-frontend watcher::tests::configured_remote_task_stats -- --ignored --nocapture
```

The backend tests cover query errors versus genuinely empty collections, Int32/Int64 counters, source precedence, active-versus-soft-deleted registration filters, task identity, monotonic versions, merged refresh requests, polling without stream events, and receiving/trailing events while another query is blocked. The optional integration check reads `asi_d1_selected4_ideas` using the application's actual routing and statistics helpers, and checks the remote replica set. It creates no data and writes no database records.

Global success, per-task failure, and discovery-failure listeners in `src/main.js` update the same guarded store, including while the task page is unmounted. Discovery-failure versions are taken before starting the snapshot, so they cannot invalidate newer successful statistics. Native Tauri event delivery/UI interactions still require an application smoke test; Vite builds and unit tests alone are not proof of that path.

## Confirmed pause versus research state

`node --test tests/*.test.mjs` exercises the real Priority SFC (SSR/event setup),
TaskManager pause handler, Pinia store and existing list-request coordinator.
A pending stop shows `stopping`; only a matching structured native receipt or
valid persisted exit evidence establishes confirmed `paused`. Void command returns,
success logs and bare paused records do not. Failed/invalid/timed-out verification
shows stop-unconfirmed, not paused. These operation states outrank cached or late
active/blocked research metadata, which is explicitly not process liveness. Verified
success commits captured ID/host-fenced evidence before fetching, without resetting
measured counters/groups. Pre/during-command reads are invalidated; subsequent
list-load failure cannot undo verified evidence. `$oid` IDs, same-name replacements, changed hosts and
unmount are covered. These are offline regressions, not native-app smoke tests.

## Exact legacy reconciliation reason compatibility

`commands.rs::dispatch_state_tests` includes a minimal sanitized fixture derived
from a live capability's **shape**, not its account/task/PID/run identity. It has
fresh UTC v3 evidence, unconfirmed budget, and null daily counters/reset. Native
accepts the raw `quota.reason` verbatim or, only for `waiting_reconciliation`, the
exact historical prefix `请求结果待核验；暂停新请求，已接受任务继续采集。` followed by that same
raw reason. The emitted DTO always carries the raw reason. Tampering, repeated
prefixes, prefix-only text, missing raw reason, wrong state/identity/target,
stale/future clocks, invalid PID/run/protocol and inconsistent quota still fail.
`submissionDisplayReason` is display-only and is not trusted as machine evidence.

Verified offline: 8 tests passed by extracting the actual pure Native helpers and
this Rust test module (excluding its Mongo deadline test), then compiling with
already-cached BSON/Serde/Chrono dependencies; the old validator failed exactly
the new legacy-acceptance regression. Also, 13 existing SFC/runtime tests passed:

```sh
node --test tests/test_dispatch_wait_state.test.mjs tests/test_regular_runtime_status.test.mjs
# Optional full-crate equivalent; not run in this verification. Uses local cache
# only, skips the Mongo endpoint test, and never invokes the application:
cd src-tauri
cargo test --offline --locked --bin quantnight-frontend dispatch_state_tests:: -- --skip unavailable_db_does_not_block_process_status_beyond_two_second_deadline
```

Cached Rust artifacts must match the compiler and dependency graph: selecting
rlibs merely by newest mtime can mix Homebrew and rustup builds and produce
E0514. Use an explicitly matched installed toolchain/cache; do not clean or
install dependencies as a diagnostic shortcut. These tests do not prove the
running Native app has loaded the patch; that requires a separately authorized
frontend build/reload, not a Python consumer restart.

## Template layout and binding migration (offline)

```sh
node --test tests/templateLayout.test.mjs tests/researchArchive.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml --offline --locked \
  --bin quantnight-frontend commands::template_catalog_tests::
# Optional cross-repo CLI contract: copy only catalog.py into a temporary fixture;
# the interpreter executes stdlib discovery, never imports template code or taskctl.
QUANTNIGHT_CATALOG_SOURCE=/path/to/quantnight/templates/catalog.py \
QUANTNIGHT_TEST_PYTHON=/path/to/quantnight/.venv/bin/python \
cargo test --manifest-path src-tauri/Cargo.toml --offline --locked \
  --bin quantnight-frontend commands::template_catalog_tests:: -- --include-ignored
node node_modules/vite/bin/vite.js build --outDir tmp/template-layout-build
```

Verified: real SFC handlers and configStore with fake Tauri IO preserve custom
paths, migrate only shipped defaults, persist unknown config keys, use task-kind
specific discovery, strip the full `.research.json` suffix, and allow manual legacy
selection after empty discovery. Real native archive/discovery consumers use a
temporary fake executable to prove CLI arguments, exact task/host archive identity,
fail-closed errors and command/task/nonempty archivePath receipt validation.
The optional real catalog test covers both kinds with the same name, descriptors,
helper/claim/temp exclusion without execution, legacy selection, collision and
manifest tampering. It changes only temporary directories and cleans them afterward.

Verified pitfalls (template-layout worker): the packaged config had relative old
defaults, different from `src/config.js` absolute defaults; updating only one misses
installed applications. Also macOS temp `/var/...` aliases `/private/var/...`:
canonicalize fixture roots before handing absolute directory paths to the catalog.
The initial alias mismatch correctly failed closed, rather than discovering an
unrelated template. Deployment/environment aliases remain a backend contract issue,
not a reason to bypass its containment checks.
A deferred fake discovery response also verifies that late auto-selection cannot
overwrite a user-entered legacy name or changed directory; initial empty-name checks
alone are insufficient once the native command is asynchronous.
