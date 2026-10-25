import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, realpathSync, watch } from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { EventEmitter } from 'node:events';
import { spawnSync } from 'node:child_process';
import { devInputs, relevant } from '../scripts/dev-inputs.mjs';
import { enrollmentPlan, writeEnrollment } from '../scripts/dev-registration.mjs';
const read = p => readFileSync(new URL(p, import.meta.url), 'utf8');

test('static early entry uses the original library before GUI config/loops; standalone graph has no desktop dependency', () => {
  const main = read('../src-tauri/src/main.rs');
  const entry = read('../crates/taskctl/src/entry.rs');
  const branch = main.slice(main.indexOf('fn main()'), main.indexOf('let config_path'));
  assert.match(branch, /args_os\(\)\.skip\(1\)/);
  assert.match(branch, /args.next\(\).as_deref\(\) == Some\(std::ffi::OsStr::new\("taskctl"\)\)/);
  assert.match(branch, /std::process::exit\(quantnight_taskctl::run_os_with_admission\(args, dev_identity::admit_cli\)\)/);
  assert.ok(entry.indexOf('admit(app_config') < entry.indexOf('let runtime ='));
  assert.ok(entry.indexOf('Ok(Entry::Help') < entry.indexOf('admit(app_config'));
  assert.match(read('../crates/taskctl/src/main.rs'), /quantnight_taskctl::run_os\(std::env::args_os\(\).skip\(1\)\)/);
  for (const crate of ['task-core', 'taskctl']) {
    assert.doesNotMatch(read(`../crates/${crate}/Cargo.toml`), /tauri|linfa|quantnight-frontend/);
  }
  const dispatch = read('../crates/taskctl/src/dispatch.rs');
  assert.match(dispatch, /StopBudget::Cli/);
  assert.match(read('../src-tauri/src/commands.rs'), /StopBudget::Gui/);
  assert.doesNotMatch(read('../src-tauri/build.rs'), /Command::|cargo (run|build)|spawn/);
});

test('existing watcher coalesces source add/change/unlink and does not consume generated output', async () => {
  const root = realpathSync(mkdtempSync(path.join(os.tmpdir(), 'qn-dev-watch-')));
  let observer;
  const server = { watcher: new EventEmitter(), httpServer: new EventEmitter() };
  let added;
  server.watcher.add = paths => { added = paths; };
  try {
    mkdirSync(path.join(root, 'src-tauri/src'), { recursive: true });
    const binding = path.join(root, 'binding.json');
    mkdirSync(path.join(root, 'backend'));
    writeEnrollment(enrollmentPlan({ frontendRoot:root, backendRoot:path.join(root,'backend'), executable:path.join(root,'host'), bindingPath:binding, target:'aarch64-apple-darwin' }));
    devInputs(root, {}).configureServer(server);
    assert.ok(added.includes(path.join(root, 'crates/taskctl/src')));
    const stamp = path.join(root, 'src-tauri/src/dev-input.stamp');
    assert.match(readFileSync(stamp, 'utf8'), / 1\n$/);
    const event = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('no coalesced stamp event')), 3000);
      observer = watch(stamp, () => { clearTimeout(timer); resolve(); });
    });
    for (const kind of ['add', 'change', 'unlink']) server.watcher.emit('all', kind, path.join(root, 'crates/task-core/src/a.rs'));
    await event;
    assert.match(readFileSync(stamp, 'utf8'), / 2\n$/);
    for (const input of ['target/debug/a.rs', 'artifacts/a.rs', 'src-tauri/src/dev-input.stamp', 'crates/taskctl/target/a.rs', 'logs/a.rs']) assert.equal(relevant(input), false, input);
    for (const input of ['crates/task-core/src/a.rs', 'crates/taskctl/src/a.rs', 'crates/Cargo.lock', 'crates/taskctl/Cargo.toml']) assert.equal(relevant(input), true, input);
  } finally {
    observer?.close(); server.httpServer.emit('close'); rmSync(root, { recursive: true, force: true });
  }
});

test('BLOCKER reproduction: host build.rs fingerprint can describe B while linked cached dependency is A', () => {
  // No project dependencies/GUI/DB: actual Cargo scheduling with two std-only
  // crates. The host build-script edit is a controlled stand-in for an editor
  // changing a dependency after Cargo has checked that cached dependency.
  const root = mkdtempSync(path.join(os.tmpdir(), 'qn-dev-build-race-'));
  try {
    mkdirSync(path.join(root, 'leaf/src'), { recursive: true });
    mkdirSync(path.join(root, 'host/src'), { recursive: true });
    writeFileSync(path.join(root, 'Cargo.toml'), '[workspace]\nmembers=["leaf","host"]\nresolver="2"\n');
    writeFileSync(path.join(root, 'leaf/Cargo.toml'), '[package]\nname="qn-fixture-leaf"\nversion="0.1.0"\nedition="2021"\n');
    writeFileSync(path.join(root, 'leaf/src/lib.rs'), 'pub fn value() -> &\'static str { "A" }\n');
    writeFileSync(path.join(root, 'host/Cargo.toml'), '[package]\nname="qn-fixture-host"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nqn-fixture-leaf={path="../leaf"}\n');
    // Keep the generated Rust unambiguous (the edit itself is the fake I/O).
    writeFileSync(path.join(root, 'host/build.rs'), String.raw`fn main() {
      std::fs::write("../leaf/src/lib.rs", r#"pub fn value() -> &'static str { "B" }
"#).unwrap();
      println!("cargo:rustc-env=EMBEDDED_SOURCE=B");
    }`);
    writeFileSync(path.join(root, 'host/src/main.rs'), 'fn main() { println!("source={} linked={}", env!("EMBEDDED_SOURCE"), qn_fixture_leaf::value()); }\n');
    const env = { ...process.env, CARGO_NET_OFFLINE: 'true', CARGO_TARGET_DIR: path.join(root, 'target') };
    const run = args => {
      const result = spawnSync('cargo', args, { cwd: root, env, encoding: 'utf8', timeout: 20000 });
      assert.equal(result.status, 0, `${result.error || ''}\n${result.stderr}`);
      return result.stdout.trim();
    };
    run(['build', '--offline', '-q', '-p', 'qn-fixture-leaf']);
    assert.equal(run(['run', '--offline', '-q', '-p', 'qn-fixture-host']), 'source=B linked=A');
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('fixed Cargo case: actual exported linked identity rejects source B / linked A through the shared Rust owner', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'qn-linked-identity-'));
  try {
    for (const p of ['leaf/src', 'host/src']) mkdirSync(path.join(root, p), { recursive: true });
    writeFileSync(path.join(root, 'source_identity.rs'), read('../crates/source_identity.rs'));
    writeFileSync(path.join(root, 'Cargo.toml'), '[workspace]\nmembers=["leaf","host"]\nresolver="2"\n[profile.dev.build-override]\ndebug=2\n');
    writeFileSync(path.join(root, 'leaf/Cargo.toml'), '[package]\nname="qn-identity-leaf"\nversion="0.1.0"\nedition="2021"\n[build-dependencies]\nsha2="0.10"\n');
    writeFileSync(path.join(root, 'leaf/build.rs'), '#[path="../source_identity.rs"] mod source_identity; fn main() { source_identity::emit_crate_identity().unwrap(); }');
    const leaf = version => `pub const ID: &str = env!("QUANTNIGHT_CRATE_SOURCE_ID"); pub fn value() -> &'static str { "${version}" }\n`;
    writeFileSync(path.join(root, 'leaf/src/lib.rs'), leaf('A'));
    writeFileSync(path.join(root, 'host/Cargo.toml'), '[package]\nname="qn-identity-host"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nsha2="0.10"\nqn-identity-leaf={path="../leaf"}\n[build-dependencies]\nqn-identity-leaf={path="../leaf"}\n');
    writeFileSync(path.join(root, 'host/build.rs'), `fn main() { assert_eq!(qn_identity_leaf::value(), "A"); std::fs::write("../leaf/src/lib.rs", r#"${leaf('B')}"#).unwrap(); }`);
    writeFileSync(path.join(root, 'host/src/main.rs'), String.raw`
#[path="../../source_identity.rs"] mod source_identity;
fn main() {
  assert_eq!(qn_identity_leaf::value(), "A");
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../leaf");
  match source_identity::admit_linked(&root, qn_identity_leaf::ID) {
    Err(e) => { eprintln!("{e}"); std::process::exit(23); }
    Ok(()) => panic!("stale source reached forbidden business sentinel"),
  }
}`);
    const env = { ...process.env, CARGO_NET_OFFLINE: 'true', CARGO_TARGET_DIR: path.join(root, 'target') };
    const run = (args, code = 0) => {
      const result = spawnSync('cargo', args, { cwd: root, env, encoding: 'utf8', timeout: 45000 });
      assert.equal(result.status, code, `${result.error || ''}\n${result.stderr}`); return result;
    };
    run(['generate-lockfile', '--offline']);
    run(['build', '--offline', '--locked', '-q', '-p', 'qn-identity-leaf']);
    const rejected = run(['run', '--offline', '--locked', '-q', '-p', 'qn-identity-host'], 23);
    assert.match(rejected.stderr, /linked crate source is stale/);
    assert.doesNotMatch(rejected.stderr, /forbidden business sentinel/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
