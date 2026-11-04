# Native taskctl — shared core, not automatically installed

For the complete command list, arguments and copyable workflows, see the
[CLI usage guide (中文)](../../docs/cli-usage.md). This file covers building,
configuration selection and development binding; the usage guide is the command reference.

All 16 original task commands share the core-owned typed request/dispatcher and
TaskControl with the desktop. With explicit runtime enrollment they are equal
clients of one headless task host; unenrolled compatibility mode keeps the original
embedded route. The CLI does not implement task/DB/receipt policy or link Tauri.
Neither client privately spawns a daemon or falls back after a runtime error.

## Shared source owners

Both adapters use `RuntimeClient` and the public
`TaskEndpoint.execute(TaskRequest, observer)` facade. The only typed requests and
business dispatcher are [task_request.rs](../task-core/src/task_request.rs) and
[task_dispatch.rs](../task-core/src/task_dispatch.rs). The shared
[runtime directory](../task-core/src/runtime/) owns protocol/codec/client/server/
identity; adapters do not copy schema or add a transport receipt service.

Binding/runtime schema parsing is owned by
[crates/runtime_registration.rs](../runtime_registration.rs), not a separate
GUI/Node parser. Native mapped-image/process identity is shared through
[crates/native_identity.rs](../native_identity.rs). The Node registration owner
delegates selection/path/plist planning and authenticated status/drain to the
original taskctl's [runtime management entry](src/runtime_entry.rs); it must not
copy the socket codec or path hash. These are source pointers, not executable
help or installed/adoption evidence.

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
the binding, then lazily constructs only the requested configured role. In runtime
enrollment that factory and validation belong to the host, not each CLI call. Missing
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

1. Runtime-enrolled mode directly execs the registered independent
   `runtime.executable` as the CLI client, without GUI files or GUI ready state.
2. Schema 1 without runtime enrollment keeps the canonical GUI executable's
   early `taskctl` entry in registered dev mode.
3. Before runtime enrollment, absolute `QUANTNIGHT_TASKCTL_BIN` remains explicit
   standalone intent when it does not conflict with registration/environment;
   a durable standalone registration permits `<backend>/bin/taskctl`.

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

## Runtime enrollment and safe adoption

This is the build/enrollment/lifecycle guide for the **runtime-enrolled source
contract**, not a claim that a checkout, installer or running host has adopted it.
Exact management command examples must match the built executable's help.
Task commands and their side effects remain in the [CLI guide](../../docs/cli-usage.md).

### One task runtime, two equal clients

On explicitly enrolled macOS installations, the original independent `taskctl`
hosts one Tokio runtime, TaskControl and shared lazy Mongo client slots. GUI task
commands, list/remote-only cache, diagnostics and progress projections, and every
CLI task command use the same core-owned Unix IPC client, codec, identity checks,
errors and dispatcher. WebView Tauri invoke is only the UI-to-GUI adapter; it is
not an external CLI socket API. The host has no Tauri/window dependency.

This moves the task runtime owner, not an imaginary per-button GUI connection:
the old GUI already reused Arc clients. Non-task Alpha/data/dataset/PNL and
correlation functions retain their original GUI data pool and settings. They are
not merged into the task host, and task adapters must not borrow that pool.

### Explicit enrollment, not automatic activation by a source edit

The original frontend/backend `.quantnight/taskctl-selection.json` locators
continue to reference one canonical non-secret binding. Schema 1 without a
runtime enrollment retains the old mode. Schema 2 extends that same document with
the explicitly selected independent runtime executable, configuration selector and
protocol version; there is no second runtime configuration authority.

The enrolled Python shim **directly execs the registered runtime executable as a
CLI client**. It does not traverse the GUI executable/trampoline or require the
GUI file, GUI ready record or an open window. A broken/stopped/stale enrolled host
does not permit embedded/direct-DB fallback, a different checkout or a release
guess. The old explicit standalone/dev selection rules apply only before runtime
enrollment; non-macOS runtime enrollment is unsupported, but the original
unenrolled non-macOS paths remain available.

The original backend MongoConfig remains endpoint authority. The selected runtime
configuration is either the canonical `backend_root` or an explicitly selected
canonical original AppConfig TOML (`app_config`). The latter uses the original
AppConfig loader → `resolve_gui_binding` → MongoConfig authority, retaining the
original task script contract, currently `bash.scripts.check_task_status`. It does
not send an AppConfig/URI/script descriptor over IPC or expose arbitrary shell RPC.
Pure CLI backend-root mode stays available, but has no invented GUI diagnostic
script defaults. A GUI must match the host's actual task settings.

Task-settings compatibility covers original backend/task namespaces and the
existing task-script descriptor, not Alpha/data/PNL/UI settings or legacy TOML
URIs. Non-task/UI-only changes therefore do not require host restart. Original
binding freshness still applies; task script/endpoint changes require safe explicit
adoption, not silent reconnection or GUI-only restart.

An executable-only schema 2 upgrade **must preserve the existing selector** unless
the operator explicitly supplies a different one. Only first schema 1 enrollment
without a candidate configuration defaults to backend_root. Before changing a
selector, stop/drain the old selection to idle/unloaded **before publishing the
new binding**; do not strand the old host behind a changed pointer. Review the
shared plan and reject an unintended selector change.

An authorized owner follows this order, using the original registration tool's
plan-only default and the real management interface:
1. Review canonical frontend/backend/binding/configuration/executable selection
   and conflicts; do not overwrite another enrollment or guess paths.
2. Build the independent `taskctl` from the Rust workspace above. This neither
   installs it nor reloads a running image. Do not bare-build a live GUI.
3. For an existing host, authenticate management against that exact host and
   request drain. It rejects new business while accepted core futures finish.
   Wait for active zero **and** original operation cleanup; a timeout does not
   authorize killing it. Only then may the original owner boot out the verified
   PID/birth/generation and explicitly write/cut over/bootstrap the selected host.
4. Verify binding, protocol/shared source, host PID/native birth/generation,
   ready state and the host's mapped-image UUID against its selected executable.
   Verify the GUI's own loaded image separately: two different executables need
   not have equal UUIDs. Record actual installation/loaded evidence outside this
   guide. A help response alone does not prove runtime adoption.

The original Node installer delegates path/label derivation and management
status/drain to the shared Rust owner through taskctl; it must not implement a
second socket codec or path-hash/identity algorithm. Management of a stale host
remains available when UID/backend/selection/protocol and real process identity
match; business admission still requires the current shared source generation.
Management cannot send a business Call.

### Management and deployment command reference

These are source-contract examples for an authorized deployment owner, not an
automatic install script or evidence of deployment. **Check the rebuilt native
executable's help first.** If its runtime-plan help lacks the source's
`--app-config` or `--metadata-only`, the executable is older: do not run the candidate/deployment
examples until the original owner builds it and verifies the complete help.
Node help alone cannot prove the forwarded Rust command supports that flag.

Use explicit canonical paths from the existing binding. `GUI_EXE` and `TARGET`
below mean that binding's unchanged GUI path/target; they are not discovered from
a running GUI. `EXPECTED_ORIGINAL` is an owner-prepared file containing the exact
original binding bytes. Do not overwrite an unrelated snapshot or enrollment.

```sh
FRONT=/canonical/frontend
BINDING=/canonical/existing/taskctl-dev-binding.json
CLI="$FRONT/crates/target/debug/taskctl"
APP_CONFIG=/canonical/existing/original/config.toml

"$CLI" --help
"$CLI" runtime-plan --help
"$CLI" runtime-status --help
"$CLI" runtime-drain --help
"$CLI" runtime-host --help
node "$FRONT/scripts/register-dev-host.mjs" --help
```

Rust management commands are **prefix-free**. The candidate `--app-config`
belongs after `runtime-plan`; it is distinct from business global
`taskctl --app-config FILE COMMAND` and from the task's `start --config JSON`.
The original Node registration tool's corresponding candidate flag is
`--runtime-app-config`; Node does not parse TOML or choose a GUI default.

For an already existing canonical binding, review only (no registration,
activation, DB call or write):

```sh
# Default plan validates original business configuration (read I/O, no DB/write/activation).
"$CLI" runtime-plan --binding "$BINDING" --runtime-executable "$CLI" \
  --app-config "$APP_CONFIG"
node "$FRONT/scripts/register-dev-host.mjs" --binding "$BINDING" \
  --runtime-action plan --runtime-executable "$CLI" \
  --runtime-app-config "$APP_CONFIG"
```

Default runtime-plan reads the original selected configuration through its
original loader/config authority and validates the business candidate, without
constructing Mongo clients, querying DB, writing enrollment or activating a
service. **Read-only is not no-I/O.** Plan JSON's `business_validated` reports
whether that validation occurred; it is not a loaded/ready or task-success fact.

For an already runtime-enrolled selection, metadata-only planning reads the
registered structure and canonical selector metadata, not the business TOML:

```sh
"$CLI" runtime-plan --binding "$BINDING" --metadata-only
```

It reports `business_validated=false`, allowing safe management of the old
selection even when its business TOML is missing/invalid; it is not permission to
start with invalid business configuration. The original Node stop/old-unloaded
checks use this metadata plan internally. Start/reload revalidate a default
business plan under the same binding-writer lock **before draining/stopping a
working host**. Validation failure must leave that working host running. Node
does not define a separate metadata codec, parser or extra public metadata flag.

For pure CLI first enrollment, omit the AppConfig candidate deliberately; do not
claim GUI diagnostic script compatibility. For an existing schema 2 executable
upgrade with no configuration change, omit the candidate and require the plan to
retain its current selector. Plan JSON comes from the shared Rust owner; Node
consumes it rather than duplicating path/label/plist/hash algorithms.

An existing schema 2 host must be stopped safely **using its old binding** before
changing the selector or publishing the upgrade:

```sh
# Mutates host lifecycle; only with explicit deployment authorization.
node "$FRONT/scripts/register-dev-host.mjs" --binding "$BINDING" \
  --runtime-action stop --write
```

Busy returns pending without bootout/kill/poll/retry. An asynchronous bootout
whose service/process exit is not yet confirmed also returns pending with
`stopRequested`; it is not a completed stop or permission to restart. Preserve
the old binding, allow accepted futures/owner cleanup to finish, and let the
deployment owner reconcile that exact generation; do not loop or publish the
new selector early.
Once stop/unloaded is confirmed, an exact-original upgrade and socket registration
may be performed explicitly:

```sh
# BACK / GUI_EXE / TARGET are unchanged canonical original binding values.
# EXPECTED_ORIGINAL already holds that binding's exact bytes.
node "$FRONT/scripts/register-dev-host.mjs" --mode dev \
  --frontend-root "$FRONT" --backend-root "$BACK" --executable "$GUI_EXE" \
  --binding "$BINDING" --target "$TARGET" --runtime-executable "$CLI" \
  --runtime-app-config "$APP_CONFIG" --expected-original "$EXPECTED_ORIGINAL" \
  --write --cutover
```

First-ever setup first explicitly creates the legacy schema 1 binding/locators
using the legacy registration section below, then performs this separate upgrade;
the runtime candidate planner requires an existing binding. Neither registration
nor cutover starts business work: cutover bootstraps the socket job, first socket
demand activates the host, and loaded identity still requires separate evidence.

For an enrolled, stopped selection, the original owner's
`--runtime-action start --write` explicitly bootstraps it. For an unchanged
selection, `--runtime-action reload --write` drains/stops before re-registering;
neither compiles binaries or changes selectors. Low-level authenticated inspection
and drain use the same Rust client:

```sh
"$CLI" runtime-status --binding "$BINDING"
# HOST_GENERATION is the exact original status identity, not a task ID or PID.
"$CLI" runtime-drain --binding "$BINDING" --generation "$HOST_GENERATION"
```

A stopped selection may leave a Unix socket pathname on macOS. Read-only
`runtime-status --binding "$BINDING" --stopped-observation` uses the original
native owner to check the exact ready/selection/UID/generation, absent original
PID, unloaded job and a refused zero-byte connection. It neither activates a
service nor sends Hello/Call, unlinks files or proves a business terminal state.
Only an explicit original `start --write` lifecycle action may reclaim that
selected stale socket: under both original locks it rechecks the native
observation and unchanged socket/ready inode, timestamps, owner/mode and absent
job before unlinking that one socket and bootstrapping. A live listener, missing
or changed evidence, reused/live PID, unknown observation or another selection
is refused. Never manually remove a socket to bypass these checks.

Drain is not bootout or business cancellation. `runtime-host` is launchd-only;
do not run it manually as a private service. Do not use `kickstart -k` to kill a
busy host. No command here authorizes simulation, MCP reload or runner restart.

### Socket activation and safe stop

The macOS user LaunchAgent in `gui/<uid>` uses socket activation on first
demand **within that user's logged-in session**, without the QuantNight GUI
window/process or MCP. This does not claim logged-out/system-daemon headless support. It uses a private Unix stream listener with UID/ownership/
permission checks; it is not an HTTP port. Listener ownership and cached listener
credentials alone do not identify the activated server: the shared identity owner
must verify the actual peer process UID/PID/native birth/executable, selected
loaded image and protocol/source/selection before business. Never generally
accept a root peer or copy a different check into GUI/CLI. The plist sets neither KeepAlive nor
RunAtLoad. The first version has no automatic idle exit. Closing the GUI only
drops its subscription/client, not the host or original simulation runner.

Explicit stop/reload must drain before bootout; never use `kickstart -k` to kill
a busy task host. Stop preserves enrollment and fails closed until the original
owner explicitly bootstraps it again. SIGTERM begins drain but cannot guarantee
completion under SIGKILL, OS crash, logout or shutdown. After a crash, the next
socket demand can restore the listener; it does not restore/replay old calls.
The independent MCP daemon, tmux/remote runners, business receipts and notification
owners keep their existing lifecycle and need no MCP reload for this change.

### Disconnects, budgets and results

From the **first attempted write of Call bytes**, a missing response is
unconfirmed, even without Accepted. Do not reconnect/resend, replay a read, fall
back to DB or infer that the operation was rejected. Only pre-Call connect/Hello
may reconnect finitely within the same selection's 10-second startup budget.
That budget does not cap a 900-second generate or any other business operation.

Accepted means in-memory host ownership, not a durable business receipt.
Detached core work survives client EOF/CLI interruption/GUI close; socket write
failure does not cancel it. The host holds only in-memory request IDs, inflight
state and operation handles. The host issues a connection-bound Call ID during
Ready, rejects replay on a different connection, and removes inflight IDs after
core cleanup; it does not accumulate completed/seen IDs or impose a lifetime
request-count limit. It persists no transport receipt/outcome/journal/operations store,
result copy or replay queue, and loads no calls after restart. Reconcile an
unconfirmed operation through the original task/generation/launch/business
receipts; report gaps in those owners rather than adding a transport ledger.

### Generation ownership and interrupted-work reconciliation

A live host PID is not the lifetime of each Generate. Original
`generation.producerPid` remains the controller PID. Before publishing begin,
the original generation business owner registers an active-only scope keyed by
exact task ID and original generation ID and holds it through writer/final-CAS
cleanup; client disconnect does not end it. Optional `generation.controlOwner`
records native PID/birth and trusted business-instance provenance, not success/
done, an RPC nonce, client completion or a transport journal.

Only the same current healthy trusted instance can establish that exact scope
is inactive. The original inspector's internal `--inspect-generation-writers`
mode still checks actual writer argv/target/birth, orphan/older writers and races;
`writer_absent` asserts writer absence only, never owner death or task stop.
Both business-scope absence and original writer absence are required before the
original task lock/reread/empty-output/previous-status/full-CAS/readback recovery
conditions may settle anything. Legacy/untagged/wrong-instance/birth/malformed,
nonempty or live-writer cases remain conservative; no metadata migration,
request replay or automatic generate/start. This is original business recovery,
not a new queue/state machine or public CLI command. Original Python watch
producerPid remains the real independent watcher child.
See [the business scope owner](../task-core/src/generation_owner.rs); detailed
lifecycle semantics remain in the compatible backend's task-control guide.

### Progress, operation context and finite management observations

Process-stage updates may be best-effort/coalesced. An explicit ProgressGap is a
non-business warning, not failed/terminal/cancelled evidence. Pending stage events
precede Completed; the final original result/error and full typed ConfirmedTaskStop
are carried separately, not reconstructed from progress or GUI time. A failure
after core execution begins cannot be relabeled not_sent.

Original per-operation operational context is captured by
[operations_runtime.rs](../task-core/src/operations_runtime.rs)'s
OperationalEnvironment, not by a second adapter whitelist. Only original necessary
PATH/proxy/SSH context reaches that operation's child processes via task-local
scope; never the whole environment, host-global mutation, credential/config
URI overrides, or platform/AI secrets. Root/PYTHONPATH/TMUX and original binding
metadata keep their authoritative owner overrides. Nothing is persisted in
plist/ready/logs; do not add a generic environment/config execution API.

HostStatus exposes fixed management observations: accepted_calls, accepted_by_kind
for the closed 20 TaskRequest kinds, successful progress_subscriptions and per-role
pools initialized/settled/actual initialization_attempts. These are finite fixed-label
counters, not per-task-name/argument/environment/result history, TCP connection
counts, candidate budgets or new dispatch limits. Management adds no DB query,
background polling/logging or configuration. They describe Rust task-host pools,
not Python professional helpers or the retained non-task GUI pool.

GUI read-only runtime diagnostics keep their original 12-second caller budget,
2-second task-registration read and 15-second UI observer as separate layers.
CallContext carries remaining caller time including connect; the host does not
start another 12 seconds. Stage identity means task-registration/target matching,
not account authentication or per-task permission. A failed check neither hides
existing detail/results nor proves the task stopped; read-only access and control
ownership remain distinct and remote-only protections are unchanged.

Short task-store reads retain **30 seconds per original read stage**, writes/CAS/
rename retain 8 seconds; generate retains 900 seconds and other original business
budgets remain unchanged. There is no 30-second total RPC/command deadline.
Bounded wire chunks are not a total result/export cap. Full export remains with
the original writer; relative output paths are resolved against caller cwd before
sending, never by changing the host's process-wide cwd. Pool reuse is not proof
that MongoDB latency/root causes are resolved. Requests/config/URIs/credentials
and arbitrary result bodies are not host logs.

No new legitimate-call total request-size cap, full-export cap, concurrency/count
admission gate or idle transport deadline is adopted to change original business
behavior. Private same-user clients are jointly trusted, **not an OS sandbox**;
resource DoS from a hand-written untrusted same-UID stream remains a residual
risk. Bounded chunks/less copying do not establish fixed total heap or protection
against a malicious same-user peer, nor prove permanent stability.

### Development generations

Stock Vite/Tauri remains the GUI build/reload owner. Its reload does not reload
the independent task host. Shared core/runtime/CLI source changes make an old
host reject new business while admitted operations finish; the original owner
explicitly builds, drains and adopts the new host. UI-only changes must not force
host restart. No client, shim or build.rs runs Cargo, launchctl or a new rebuild
broker. On-disk source/binary freshness, each loaded image and actual business
verification are separate evidence.

<a id="explicit-desktop-development-taskctl-mode"></a>

## Legacy desktop development taskctl mode (schema 1 only)

Before runtime enrollment, a dev-selected GUI executable retains its original
early `taskctl` entry, before GUI configuration/windows; the OS loader still
loads desktop frameworks. This compatibility mode is not the independent host.
Its fixed locators, explicit standalone choice and fail-closed enrollment checks
remain; missing enrollment does not silently authorize `<backend>/bin/taskctl`.

For legacy dev registration only, the original plan command is:

```sh
node scripts/register-dev-host.mjs --mode dev \
  --frontend-root /canonical/frontend --backend-root /canonical/backend \
  --executable /canonical/target/debug/quantnight-frontend \
  --binding /canonical/existing-parent/dev-binding.json --target aarch64-apple-darwin
# Explicit setup authorization is required before adding --write.
```

The backend root must match the original AppConfig owner. Conflicting locators
are never overwritten through environment overrides. In this legacy mode,
stock Vite/Tauri source/build identity, mapped GUI UUID, process birth/lock and
ready generation remain the original admission checks. A rejected dev lifecycle
must be repaired normally, not bypassed by retrying a task. Tests use isolated
fixtures, not real enrollment. Once runtime is enrolled, the independent client
path above replaces this GUI trampoline; GUI ready is not host authority.
