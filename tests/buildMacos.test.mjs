import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, symlinkSync, rmSync, existsSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const shell = readFileSync(new URL('../scripts/build-macos.sh', import.meta.url), 'utf8');
// Every packaging command is a fake executable. PATH contains ONLY these and
// explicit basic filesystem/text utilities: no real npm/brew/Mach-O tools can run.
const fake = String.raw`#!${process.execPath}
const fs = require('node:fs');
const path = require('node:path');
const root = process.env.FIXTURE_ROOT;
const args = process.argv.slice(2), tool = path.basename(process.argv[1]);
const stateFile = path.join(root, 'state.json');
const state = JSON.parse(fs.readFileSync(stateFile, 'utf8'));
const target = process.env.CARGO_TARGET_DIR;
const app = path.join(target, 'release/bundle/macos/QuantNight.app');
const exe = path.join(app, 'Contents/MacOS/quantnight-frontend');
const lib = path.join(app, 'Contents/Frameworks/libiconv.2.dylib');
const prefix = path.join(root, 'brew prefix');
const src = path.join(prefix, 'lib/libiconv.2.dylib');
const bundled = '@rpath/libiconv.2.dylib';
const rpath = '@executable_path/../Frameworks';
const fail = (msg) => { console.error(msg); process.exit(87); };
const event = { tool, args, cwd: process.cwd() };
if (process.cwd() !== root) fail('unexpected cwd');
if (!target.startsWith(root + '/')) fail('output escaped fixture');
for (const arg of args) if (arg.startsWith('/') && !arg.startsWith(root + '/')) fail('argument escaped fixture');
if (tool === 'npm') event.environment = {
  offline: process.env.CARGO_NET_OFFLINE, npmOffline: process.env.npm_config_offline,
  noBrewUpdate: process.env.HOMEBREW_NO_AUTO_UPDATE, target,
  signing: process.env.APPLE_SIGNING_IDENTITY,
  notaryPresent: ['APPLE_ID','APPLE_PASSWORD','APPLE_TEAM_ID','APPLE_API_KEY','APPLE_API_ISSUER','APPLE_API_KEY_PATH'].some(k => k in process.env),
};
fs.appendFileSync(path.join(root, 'events.jsonl'), JSON.stringify(event) + '\n');
function step(label) {
  if (process.env.FAIL_AT === label) {
    fs.appendFileSync(path.join(root, 'events.jsonl'), JSON.stringify({ failure: label }) + '\n');
    fail('injected ' + label);
  }
}
function save() { fs.writeFileSync(stateFile, JSON.stringify(state)); }
function listing(file, deps) {
  console.log(file + ':');
  for (const dep of deps) console.log('\t' + dep + ' (compatibility version 7.0.0, current version 9.1.0)');
}
if (tool === 'npm') {
  step('npm');
  if (JSON.stringify(args) !== JSON.stringify(['run','tauri','--','build','--bundles','app','--','--locked','--offline'])) fail('wrong npm args');
  if (process.env.SCENARIO === 'missing-output') process.exit(0);
  const actual = process.env.SCENARIO === 'wrong-product' ? app.replace('QuantNight.app','quantnight-frontend.app') : app;
  fs.mkdirSync(path.join(actual,'Contents/MacOS'), { recursive: true });
  fs.writeFileSync(path.join(actual,'Contents/MacOS/quantnight-frontend'),'synthetic executable, never launched');
  if (process.env.SCENARIO === 'existing-lib') {
    fs.mkdirSync(path.dirname(lib), { recursive: true }); fs.writeFileSync(lib,'unexpected nested file');
  }
} else if (tool === 'brew') {
  step('brew');
  if (JSON.stringify(args) !== JSON.stringify(['--prefix','libiconv'])) fail('wrong brew command');
  console.log(process.env.SCENARIO === 'bad-prefix' ? 'relative/prefix' : prefix);
} else if (tool === 'otool') {
  const [flag, file] = args;
  if (!fs.existsSync(file)) fail('inspection of missing file');
  if (flag === '-L') {
    if (file === exe) {
      step(state.exeReads ? 'final-exe-deps' : 'exe-deps'); state.exeReads++; save();
      listing(file, state.exeDeps);
    } else if (file === src || file === lib) {
      step(file === src ? 'source-deps' : 'final-lib-deps');
      listing(file, [file === src ? state.sourceId : state.libId, ...state.libDeps]);
    } else fail('unknown dependency target');
  } else if (flag === '-l' && file === exe) {
    step(state.rpathReads ? 'final-rpaths' : 'rpaths'); state.rpathReads++; save();
    console.log(file + ':');
    for (const item of state.rpaths) console.log('Load command 1\n          cmd LC_RPATH\n      cmdsize 56\n         path ' + item + ' (offset 12)');
  } else if (flag === '-D' && (file === src || file === lib)) {
    step(file === src ? 'source-id' : 'final-lib-id');
    console.log(file + ':\n' + (file === src ? state.sourceId : state.libId));
  } else fail('unexpected otool args');
} else if (tool === 'install_name_tool') {
  const [op, from, to, file] = args;
  const label = { '-id':'id', '-change':'change', '-add_rpath':'rpath' }[op];
  if (!label) fail('unknown install_name operation');
  step(label);
  if (process.env.SCENARIO === label + '-noop') process.exit(0);
  if (op === '-id' && from === bundled && to === lib) state.libId = from;
  else if (op === '-change' && file === exe && to === bundled && state.exeDeps.includes(from)) state.exeDeps = state.exeDeps.map(x => x === from ? to : x);
  else if (op === '-add_rpath' && from === rpath && to === exe && !state.rpaths.includes(from)) state.rpaths.push(from);
  else fail('incorrect or duplicate install_name operation');
  save();
} else if (tool === 'codesign') {
  const file = args.at(-1), signing = args[0] === '--force';
  if (file !== lib && file !== app) fail('unexpected sign target');
  const label = (signing ? 'sign-' : 'verify-') + (file === lib ? 'lib' : 'app');
  step(label);
  const expected = signing ? ['--force','--sign','-',file] : file === app ? ['--verify','--deep','--strict',file] : ['--verify','--strict',file];
  if (JSON.stringify(args) !== JSON.stringify(expected)) fail('wrong signing/verification flags');
} else fail('unexpected command');
`;

function run({ scenario = '', failAt = '', dependency = 'brew', rpaths = [], extraDeps = [], libDeps = [], sourceId, target = '', stale = false, overrides = {}, args = [] } = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'quantnight build [test] ')));
  try {
    const bin = join(root, 'fake-bin');
    mkdirSync(bin); mkdirSync(join(root, 'scripts'));
    writeFileSync(join(root, 'scripts/build-macos.sh'), shell);
    for (const name of ['npm', 'brew', 'otool', 'install_name_tool', 'codesign']) writeFileSync(join(bin, name), fake, { mode: 0o755 });
    for (const [name, real] of [['dirname','/usr/bin/dirname'], ['awk','/usr/bin/awk'], ['mkdir','/bin/mkdir'], ['cp','/bin/cp']]) symlinkSync(real, join(bin, name));
    const prefix = join(root, 'brew prefix'), source = join(prefix, 'lib/libiconv.2.dylib');
    const physicalSource = join(root, 'brew cellar/lib/libiconv.2.dylib');
    if (scenario === 'physical-homebrew') {
      mkdirSync(join(root, 'brew cellar/lib'), { recursive: true }); mkdirSync(prefix);
      symlinkSync(join(root, 'brew cellar/lib'), join(prefix, 'lib'));
      sourceId = physicalSource;
    } else mkdirSync(join(prefix, 'lib'), { recursive: true });
    if (scenario !== 'missing-library') writeFileSync(source, 'synthetic library, never loaded');
    const dep = dependency === 'brew' ? source : dependency === 'physical' ? physicalSource : dependency === 'rpath' ? '@rpath/libiconv.2.dylib' : dependency === 'system' ? '/usr/lib/libiconv.2.dylib' : dependency;
    writeFileSync(join(root, 'state.json'), JSON.stringify({
      exeReads: 0, rpathReads: 0, exeDeps: [dep, '/usr/lib/libSystem.B.dylib', ...extraDeps],
      rpaths, sourceId: sourceId || source, libId: sourceId || source,
      libDeps: ['/usr/lib/libSystem.B.dylib', ...libDeps],
    }));
    const targetValue = target === 'absolute' ? join(root, 'external target') : target;
    const resolvedTarget = !targetValue ? join(root, 'src-tauri/target') : targetValue.startsWith('/') ? targetValue : join(root, targetValue);
    const app = join(resolvedTarget, 'release/bundle/macos/QuantNight.app');
    if (stale) { mkdirSync(app, { recursive: true }); writeFileSync(join(app, 'sentinel'), 'keep'); }
    const env = {
      PATH: bin, HOME: root, FIXTURE_ROOT: root, SCENARIO: scenario, FAIL_AT: failAt,
      APPLE_ID: 'synthetic', APPLE_PASSWORD: 'synthetic', APPLE_TEAM_ID: 'synthetic',
      APPLE_API_KEY: 'synthetic', APPLE_API_ISSUER: 'synthetic', APPLE_API_KEY_PATH: 'synthetic',
      ...(targetValue ? { CARGO_TARGET_DIR: targetValue } : {}), ...overrides,
    };
    const result = spawnSync('/bin/bash', [join(root, 'scripts/build-macos.sh'), ...args], {
      cwd: tmpdir(), env, encoding: 'utf8', timeout: 10000,
    });
    assert.ifError(result.error);
    const events = existsSync(join(root, 'events.jsonl')) ? readFileSync(join(root, 'events.jsonl'), 'utf8').trim().split('\n').map(JSON.parse) : [];
    const state = JSON.parse(readFileSync(join(root, 'state.json'), 'utf8'));
    return { ...result, root, app, events, state, stalePreserved: stale && readFileSync(join(app, 'sentinel'), 'utf8') === 'keep' };
  } finally { rmSync(root, { recursive: true, force: true }); }
}
const signs = r => r.events.filter(e => e.tool === 'codesign');
const edits = r => r.events.filter(e => e.tool === 'install_name_tool');

// Source identity checks accompany execution of the shipped shell, not a copied
// rewrite of its logic. Public build manifests only; no user config is opened.
test('script bundle/executable names match the shipped public build metadata', () => {
  const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
  const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
  assert.equal(config.productName, 'QuantNight');
  assert.match(cargo, /\[package\]\s+name = "quantnight-frontend"/);
  assert.match(shell, /bundle\/macos\/QuantNight\.app/);
  assert.match(shell, /Contents\/MacOS\/quantnight-frontend/);
});

for (const target of ['', 'absolute', 'relative output']) test(`real shell builds exact product layout and signs after edits (target=${target || 'default'})`, () => {
  const r = run({ target });
  assert.equal(r.status, 0, r.stderr);
  const npm = r.events[0];
  assert.equal(npm.tool, 'npm');
  assert.deepEqual(npm.args, ['run','tauri','--','build','--bundles','app','--','--locked','--offline']);
  assert.deepEqual(npm.environment, { offline: 'true', npmOffline: 'true', noBrewUpdate: '1', target: r.app.split('/release/bundle/')[0], signing: '-', notaryPresent: false });
  assert.ok(r.events.every(e => e.cwd === r.root));
  assert.deepEqual(edits(r).map(e => e.args[0]), ['-id','-change','-add_rpath']);
  assert.equal(r.state.exeDeps[0], '@rpath/libiconv.2.dylib');
  assert.deepEqual(r.state.rpaths, ['@executable_path/../Frameworks']);
  assert.equal(r.state.libId, '@rpath/libiconv.2.dylib');
  assert.deepEqual(signs(r).map(e => e.args.slice(0, -1)), [
    ['--force','--sign','-'], ['--verify','--strict'], ['--force','--sign','-'], ['--verify','--deep','--strict'],
  ]);
  assert.ok(r.events.findIndex(e => e.tool === 'codesign') > r.events.findLastIndex(e => e.tool === 'install_name_tool'));
  assert.equal(signs(r)[2].args.at(-1), r.app);
  assert.match(r.stdout, /not installed or launched/);
});

test('existing correct RPATH and bundled dependency are inspected, not blindly re-added or rewritten', () => {
  const r = run({ dependency: 'rpath', rpaths: ['@executable_path/../Frameworks'], sourceId: '@rpath/libiconv.2.dylib' });
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(edits(r).map(e => e.args[0]), ['-id']);
  assert.equal(signs(r).length, 4);
});

test('Homebrew physical Cellar install name is verified through the original prefix, not assumed equal to its symlink', () => {
  const r = run({ scenario: 'physical-homebrew', dependency: 'physical' });
  assert.equal(r.status, 0, r.stderr);
  assert.match(edits(r).find(e => e.args[0] === '-change').args[1], /brew cellar\/lib\/libiconv\.2\.dylib$/);
  assert.equal(signs(r).length, 4);
});

test('system iconv stays system-owned; app alone is finally signed/strictly verified', () => {
  const r = run({ dependency: 'system' });
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(edits(r), []);
  assert.ok(!r.events.some(e => e.tool === 'brew'));
  assert.equal(signs(r).length, 2);
  assert.deepEqual(signs(r)[1].args, ['--verify','--deep','--strict',r.app]);
});

for (const opts of [
  { stale: true }, { overrides: { CARGO_BUILD_TARGET: 'unexpected-target' } },
  { overrides: { TAURI_CONFIG: '{}' } }, { args: ['--debug'] },
]) test(`reject ambiguous/stale inputs before any tool call: ${JSON.stringify(opts)}`, () => {
  const r = run(opts); assert.notEqual(r.status, 0); assert.deepEqual(r.events, []);
  if (opts.stale) assert.equal(r.stalePreserved, true);
});

for (const opts of [
  { scenario: 'wrong-product' }, { scenario: 'missing-output' }, { scenario: 'missing-library' },
  { scenario: 'bad-prefix' }, { scenario: 'existing-lib' },
  { dependency: '/unexpected/lib/libiconv.2.dylib' }, { dependency: '/usr/lib/libiconv.3.dylib' },
  { extraDeps: ['/unexpected/libother.dylib'] }, { libDeps: ['/unexpected/libother.dylib'] },
  { sourceId: '/unexpected/lib/libiconv.2.dylib' },
  { rpaths: ['/unexpected/library/path'] },
  { rpaths: ['@executable_path/../Frameworks', '@executable_path/../Frameworks'] },
  { extraDeps: ['@rpath/libiconv.2.dylib'] },
]) test(`unexpected layout/dependency refuses without signing: ${JSON.stringify(opts)}`, () => {
  const r = run(opts); assert.notEqual(r.status, 0); assert.deepEqual(signs(r), []);
  assert.doesNotMatch(r.stdout, /Verified local ad-hoc app/);
});

for (const scenario of ['id-noop','change-noop','rpath-noop']) test(`verify actual post-edit graph, not merely exit zero: ${scenario}`, () => {
  const r = run({ scenario }); assert.notEqual(r.status, 0); assert.deepEqual(signs(r), []);
  assert.match(r.stderr, /verification failed/);
});

for (const failAt of ['npm','brew','exe-deps','rpaths','source-id','source-deps','id','change','rpath',
  'final-exe-deps','final-rpaths','final-lib-id','final-lib-deps','sign-lib','verify-lib','sign-app','verify-app']) {
  test(`propagate ${failAt} error; no success/fallback signing after failure`, () => {
    const r = run({ failAt });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, new RegExp('injected ' + failAt));
    assert.deepEqual(r.events.at(-1), { failure: failAt }, 'no packaging command runs after the injected failure');
    assert.doesNotMatch(r.stdout, /Verified local ad-hoc app/);
    const expectedSignCount = { 'sign-lib': 1, 'verify-lib': 2, 'sign-app': 3, 'verify-app': 4 }[failAt] || 0;
    assert.equal(signs(r).length, expectedSignCount);
  });
}
