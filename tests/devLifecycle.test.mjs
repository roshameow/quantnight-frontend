// Real installed Tauri 2.4.1 + real existing Vite watcher, disposable std-only
// Rust leaf. No production GUI, config, DB, socket server or dependency install.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, realpathSync, chmodSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import path from 'node:path';
import os from 'node:os';
import { pathToFileURL } from 'node:url';
import { devInputs, tauriInputIgnore } from '../scripts/dev-inputs.mjs';
import { enrollmentPlan, writeEnrollment } from '../scripts/dev-registration.mjs';

test('real Tauri 2.4.1 accepts stamp and kills/waits/restarts for Vite external add/change/unlink', { timeout: 55000 }, async () => {
  const cli = process.env.QUANTNIGHT_TEST_TAURI_CLI;
  const vite = process.env.QUANTNIGHT_TEST_VITE;
  assert.ok(cli && path.isAbsolute(cli), 'explicit installed CLI path required');
  assert.ok(vite && path.isAbsolute(vite), 'explicit existing Vite module path required');
  const pkg = JSON.parse(readFileSync(path.join(path.dirname(cli), 'package.json'), 'utf8'));
  assert.equal(pkg.version, '2.4.1');
  const { createServer } = await import(pathToFileURL(vite).href);
  const root = realpathSync(mkdtempSync(path.join(os.tmpdir(), 'qn-tauri-lifecycle-')));
  let server, child, currentPid;
  const pids = [];
  const allOwnedPids = new Set();
  let groupCleaned = false;
  const cleanupGroup = () => { if (child?.pid && !groupCleaned) { groupCleaned=true; try { process.kill(-child.pid,'SIGKILL'); } catch (e) { if (e.code !== 'ESRCH') throw e; } } };
  process.once('exit',cleanupGroup);
  const messages = [];
  const events = new EventEmitter();
  let exited = false;
  const waitFor = (predicate, timeout = 12000) => new Promise((resolve, reject) => {
    const found = messages.find(predicate);
    if (found) { resolve(found); return; }
    const finish = () => { clearTimeout(timer); events.off('line', onLine); events.off('exit', onExit); };
    const onLine = line => { if (predicate(line)) { finish(); resolve(line); } };
    const onExit = () => { finish(); reject(new Error(`Tauri exited before event\n${messages.join('\n')}`)); };
    const timer = setTimeout(() => { finish(); reject(new Error(`fixture event deadline\n${messages.join('\n')}`)); }, timeout);
    events.on('line', onLine); events.on('exit', onExit);
  });
  const settledReady = () => new Promise((resolve,reject) => {
    let quiet;
    const done = () => {clearTimeout(quiet);clearTimeout(bound);events.off('line',changed);};
    const settled = () => {done();for(const pid of allOwnedPids) if(!pids.includes(pid)) pids.push(pid);resolve(messages.filter(s=>s.startsWith('LEAF_READY ')).at(-1));};
    const changed = line => {if(line.startsWith('LEAF_READY ') || line.includes('Rebuilding application')) {clearTimeout(quiet);quiet=setTimeout(settled,1800);}};
    const bound=setTimeout(()=>{done();reject(new Error('stock event burst did not settle; possible output loop'));},6500);
    events.on('line',changed);quiet=setTimeout(settled,1800);
  });
  try {
    for (const dir of ['src-tauri/src', 'crates/task-core/src', 'crates/taskctl/src', 'web', 'home', 'backend', 'fixture-bin']) mkdirSync(path.join(root, dir), { recursive: true });
    writeFileSync(path.join(root, 'src-tauri/Cargo.toml'), '[package]\nname="qn-tauri-leaf"\nversion="0.1.0"\nedition="2021"\n');
    writeFileSync(path.join(root, 'src-tauri/tauri.conf.json'), JSON.stringify({ productName: 'DisposableLeaf', version: '0.1.0', identifier: 'fixture.quantnight.dev',
      build: { frontendDist: '../web' }, app: { windows: [] }, bundle: { active: false } }));
    writeFileSync(path.join(root, 'src-tauri/build.rs'), 'fn main() { println!("cargo:rerun-if-changed=src/dev-input.stamp"); let s=std::fs::read_to_string("src/dev-input.stamp").unwrap(); println!("cargo:rustc-env=LEAF_GENERATION={}",s.trim()); }');
    writeFileSync(path.join(root, 'src-tauri/src/main.rs'), String.raw`
extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
fn main() {
  if let Ok(text) = std::fs::read_to_string("../previous-pid") {
    let old: i32 = text.trim().parse().unwrap();
    assert_ne!(unsafe { kill(old, 0) }, 0, "OVERLAP: old child still live when new generation starts");
  }
  println!("LEAF_READY {} {}", std::process::id(), env!("LEAF_GENERATION"));
  loop { std::thread::park(); }
}`);
    writeFileSync(path.join(root, '.gitignore'), 'target\n/src-tauri/src/dev-input.stamp\n');
    writeFileSync(path.join(root, 'src-tauri/.taurignore'), readFileSync(new URL('../src-tauri/.taurignore', import.meta.url)));
    const binding = path.join(root, 'binding.json');
    writeEnrollment(enrollmentPlan({frontendRoot:root,backendRoot:path.join(root,'backend'),executable:path.join(root,'leaf-host'),bindingPath:binding,target:'aarch64-apple-darwin'}));
    const scripts = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).scripts;
    writeFileSync(path.join(root,'package.json'), JSON.stringify({scripts:{tauri:scripts.tauri}}));
    const wrapper = path.join(root,'fixture-bin/tauri');
    writeFileSync(wrapper, `#!${process.execPath}\nrequire('node:fs').writeFileSync(${JSON.stringify(path.join(root,'entry-trace.json'))},JSON.stringify({argv:process.argv.slice(2),dev:process.env.QUANTNIGHT_DEV_BINDING??null,standalone:process.env.QUANTNIGHT_TASKCTL_BIN??null}));\nrequire(${JSON.stringify(cli)});\n`);
    chmodSync(wrapper,0o700);
    server = await createServer({ configFile: false, root, logLevel: 'silent',
      plugins: [devInputs(root, {})],
      server: { middlewareMode: true, ws: false, hmr: false, watch: { ignored: [tauriInputIgnore(root)] } } });
    const env = { ...process.env, CARGO_NET_OFFLINE: 'true', npm_config_offline: 'true', npm_config_update_notifier: 'false', CARGO_TARGET_DIR: path.join(root, 'target'), HOME: path.join(root, 'home'), PATH:path.join(root,'fixture-bin')+path.delimiter+process.env.PATH };
    delete env.QUANTNIGHT_DEV_BINDING;
    delete env.QUANTNIGHT_TASKCTL_BIN;
    child = spawn('npm', ['run', 'tauri', 'dev', '--', '--no-dev-server'], { cwd: root, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    for (const stream of [child.stdout, child.stderr]) {
      let pending = '';
      stream.on('data', data => {
        pending += data.toString();
        const lines = pending.split('\n'); pending = lines.pop();
        for (const raw of lines) {
          const line = raw.replace(/\x1b\[[0-9;]*m/g, ''); messages.push(line); if (line.startsWith('LEAF_READY ')) { const pid=Number(line.split(' ')[1]); allOwnedPids.add(pid); writeFileSync(path.join(root,'previous-pid'),String(pid)); } events.emit('line', line);
        }
      });
    }
    child.once('exit', () => { exited = true; events.emit('exit'); });
    await waitFor(line => line.includes('Watching ') && line.includes('src-tauri'));
    let ready = await waitFor(line => line.startsWith('LEAF_READY '));
    ready = await settledReady(); currentPid = Number(ready.split(' ')[1]); pids.push(currentPid);
    assert.deepEqual(JSON.parse(readFileSync(path.join(root,'entry-trace.json'),'utf8')), {argv:['dev','--no-dev-server'],dev:null,standalone:null});
    const external = path.join(root, 'crates/task-core/src/event.rs');
    for (const action of ['add', 'change', 'unlink']) {
      const oldPid = currentPid;
      const oldGeneration = ready.split(' ').slice(2).join(' ');
      writeFileSync(path.join(root, 'previous-pid'), String(oldPid));
      if (action === 'unlink') rmSync(external);
      else writeFileSync(external, `// external Rust ${action}\n`);
      ready = await waitFor(line => line.startsWith('LEAF_READY ') && Number(line.split(' ')[1]) !== oldPid &&
        !pids.includes(Number(line.split(' ')[1])));
      ready = await settledReady(); currentPid = Number(ready.split(' ')[1]); pids.push(currentPid);
      assert.notEqual(ready.split(' ').slice(2).join(' '), oldGeneration);
      assert.throws(() => process.kill(oldPid, 0), { code: 'ESRCH' });
      console.log(`Tauri2.4.1 ${action}: ${oldPid} exited before ${currentPid}; ${ready}`);
    }
    const lockOnlyOld = currentPid;
    const lockOnlyGeneration = ready.split(' ').slice(2).join(' ');
    writeFileSync(path.join(root,'previous-pid'),String(currentPid));
    const guiLock = path.join(root,'src-tauri/Cargo.lock');
    writeFileSync(guiLock, readFileSync(guiLock,'utf8')+'\n# only GUI lock source input changed\n');
    ready = await waitFor(line => line.startsWith('LEAF_READY ') && !pids.includes(Number(line.split(' ')[1])));
    ready = await settledReady(); currentPid=Number(ready.split(' ')[1]);pids.push(currentPid);
    assert.notEqual(ready.split(' ').slice(2).join(' '),lockOnlyGeneration);
    assert.throws(() => process.kill(lockOnlyOld,0),{code:'ESRCH'});
    console.log(`GUI Cargo.lock only: ${lockOnlyOld} -> ${currentPid}; ${ready}`);
    // Rust also records mtime: even identical-byte writes must not be hidden by
    // a Node content cache, or admission could stay stale without a rebuild.
    const touchOld=currentPid;
    const touchGeneration=ready.split(' ').slice(2).join(' ');
    writeFileSync(guiLock,readFileSync(guiLock));
    ready=await waitFor(line=>line.startsWith('LEAF_READY ')&&!pids.includes(Number(line.split(' ')[1])));
    ready=await settledReady();currentPid=Number(ready.split(' ')[1]);pids.push(currentPid);
    assert.notEqual(ready.split(' ').slice(2).join(' '),touchGeneration);
    assert.throws(()=>process.kill(touchOld,0),{code:'ESRCH'});
    console.log(`GUI lock identical-byte mtime change: ${touchOld} -> ${currentPid}`);
    const readyCount=messages.filter(line => line.startsWith('LEAF_READY ')).length;
    mkdirSync(path.join(root,'src-tauri/gen/schemas'),{recursive:true});
    writeFileSync(path.join(root,'src-tauri/gen/schemas/fixture.json'),'{}');
    await new Promise(resolve => setTimeout(resolve,1800)); // bounded negative assertion, not agent polling
    assert.equal(messages.filter(line => line.startsWith('LEAF_READY ')).length,readyCount,'generated outputs must not trigger a loop');
    assert.equal(messages.some(line => line.includes('OVERLAP')), false);
    // Stock compile failure keeps the CLI watcher alive; fixing the source
    // restarts its leaf. No stderr spoofing or new retry owner is involved.
    const source = path.join(root, 'src-tauri/src/main.rs');
    const original = readFileSync(source, 'utf8');
    const failedPid = currentPid;
    writeFileSync(path.join(root, 'previous-pid'), String(failedPid));
    writeFileSync(source, 'this is deliberately invalid Rust in a disposable fixture');
    await waitFor(line => line.includes('could not compile'));
    assert.equal(exited, false);
    assert.throws(() => process.kill(failedPid, 0), { code: 'ESRCH' });
    writeFileSync(source, original);
    ready = await waitFor(line => line.startsWith('LEAF_READY ') && !pids.includes(Number(line.split(' ')[1])));
    currentPid = Number(ready.split(' ')[1]); pids.push(currentPid);
    console.log(`Stock compile-error recovery: ${failedPid} -> ${currentPid}`);
    // A real runtime/startup error is NormalExit, not compilation failure.
    // Main explicitly selected this fail-closed lifecycle termination contract.
    const stopped = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('runtime failure did not terminate stock dev lifecycle')), 12000);
      child.once('exit', code => { clearTimeout(timer); resolve(code); });
    });
    writeFileSync(source, 'fn main() { eprintln!("fixture startup admission refused: fix and rerun npm run tauri dev"); std::process::exit(23); }');
    assert.equal(await stopped, 23);
    assert.ok(messages.some(line => line.includes('fixture startup admission refused')));
    assert.throws(() => process.kill(currentPid, 0), { code: 'ESRCH' });
    currentPid = undefined;
    console.log('Stock runtime/startup failure: dev CLI exited 23, explicit rerun required');
  } finally {
    // The current owned leaf exiting lets stock Tauri close its Cargo/dev owner.
    if (currentPid) { try { process.kill(currentPid, 'SIGTERM'); } catch (e) { if (e.code !== 'ESRCH') throw e; } }
    if (child && !exited) await new Promise(resolve => {
      const timer = setTimeout(() => { child.kill('SIGKILL'); resolve(); }, 2500);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
    });
    cleanupGroup(); process.off('exit',cleanupGroup);
    await server?.close();
    rmSync(root, { recursive: true, force: true });
    console.log('Owned fixture leaf PIDs:', JSON.stringify([...allOwnedPids]));
    if (process.env.QUANTNIGHT_LIFECYCLE_LOG) writeFileSync(process.env.QUANTNIGHT_LIFECYCLE_LOG, messages.join('\n'));
  }
});
