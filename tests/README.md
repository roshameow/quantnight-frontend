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
