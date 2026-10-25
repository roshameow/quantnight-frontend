import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync,mkdirSync,realpathSync,readFileSync,writeFileSync,rmSync, lstatSync, readdirSync, symlinkSync, readlinkSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import os from 'node:os';
import { enrollmentPlan,writeEnrollment,resolveFrontend,LOCATOR } from '../scripts/dev-registration.mjs';

test('explicit one-time metadata enrollment resolves from independent env and refuses ambiguous/conflicting selection', () => {
  const root=realpathSync(mkdtempSync(path.join(os.tmpdir(),'qn-registration-')));
  try {
    const front=path.join(root,'frontend'),back=path.join(root,'backend');mkdirSync(front);mkdirSync(back);
    const binding=path.join(root,'binding.json');
    assert.throws(()=>resolveFrontend(front,{}),/enrollment unavailable/);
    const plan=enrollmentPlan({frontendRoot:front,backendRoot:back,executable:path.join(front,'host'),bindingPath:binding,target:'aarch64-apple-darwin'});
    assert.equal(plan.length,3);writeEnrollment(plan);writeEnrollment(plan); // exact idempotence only
    assert.equal(resolveFrontend(front,{}).path,binding);
    assert.equal(resolveFrontend(front,{}).binding.backend_root,back);
    for(const dir of [front,back]) assert.equal(JSON.parse(readFileSync(path.join(dir,LOCATOR))).binding,binding);
    assert.throws(()=>resolveFrontend(front,{QUANTNIGHT_DEV_BINDING:path.join(root,'other.json')}),/conflicts/);
    assert.throws(()=>resolveFrontend(front,{QUANTNIGHT_TASKCTL_BIN:path.join(back,'bin/taskctl')}),/conflicts/);
    const saved=readFileSync(path.join(front,LOCATOR));rmSync(path.join(front,LOCATOR));
    assert.throws(()=>resolveFrontend(front,{QUANTNIGHT_DEV_BINDING:binding}),/enrollment unavailable/);
    writeFileSync(path.join(front,LOCATOR),saved);
    writeFileSync(path.join(back,LOCATOR),JSON.stringify({schema_version:1,mode:'standalone'}));
    assert.throws(()=>resolveFrontend(front,{}),/same canonical binding/);
    assert.throws(()=>writeEnrollment(plan),/conflicting existing enrollment/);
    writeFileSync(path.join(front,LOCATOR),JSON.stringify({schema_version:1,mode:'standalone'}));
    assert.equal(resolveFrontend(front,{}),null);
    assert.throws(()=>resolveFrontend(front,{QUANTNIGHT_DEV_BINDING:binding}),/conflicts/);
    writeFileSync(path.join(front,LOCATOR),'broken');assert.throws(()=>resolveFrontend(front,{}),/unavailable/);
  } finally {rmSync(root,{recursive:true,force:true});}
});

const cli = fileURLToPath(new URL('../scripts/register-dev-host.mjs', import.meta.url));
function fixture(t) {
  const root = realpathSync(mkdtempSync(path.join(os.tmpdir(), 'qn-enrollment-')));
  t.after(() => rmSync(root, { recursive:true, force:true }));
  const front = path.join(root, 'frontend'), back = path.join(root, 'backend');
  mkdirSync(front); mkdirSync(back);
  const binding = path.join(root, 'binding.json'); // canonical document outside either checkout
  const plan = enrollmentPlan({ frontendRoot:front, backendRoot:back, executable:path.join(front,'host'), bindingPath:binding, target:'aarch64-apple-darwin' });
  return { root, front, back, binding, plan };
}
function runCLI(f, { write = true, setup = '' } = {}) {
  // Faults are injected only into this disposable child's lowest filesystem I/O.
  // The real CLI, argument parser, plan and writer still execute unchanged.
  const preload = `import fs from 'node:fs'; import { syncBuiltinESMExports } from 'node:module';
    const destination = ${JSON.stringify(f.binding)};
    ${setup}
    syncBuiltinESMExports();`;
  const result = spawnSync(process.execPath, ['--import', `data:text/javascript,${encodeURIComponent(preload)}`, cli,
    '--frontend-root', f.front, '--backend-root', f.back, '--executable', path.join(f.front,'host'),
    '--binding', f.binding, '--target', 'aarch64-apple-darwin', ...(write ? ['--write'] : [])],
    { encoding:'utf8', timeout:10000, env:{ ...process.env, NODE_OPTIONS:'' } });
  assert.ifError(result.error);
  return result;
}
const entryState = p => {
  const s = lstatSync(p);
  return { dev:s.dev, ino:s.ino, mode:s.mode, size:s.size, mtime:s.mtimeMs,
    content:s.isSymbolicLink() ? readlinkSync(p) : s.isFile() ? readFileSync(p,'utf8') : null };
};
const absent = p => assert.equal(lstatSync(p, { throwIfNoEntry:false }), undefined, p);

test('original CLI default plan makes no writes; explicit enrollment is byte/permission/inode idempotent', t => {
  const f = fixture(t);
  const before = readdirSync(f.root, { recursive:true });
  const planned = runCLI(f, { write:false });
  assert.equal(planned.status, 0, planned.stderr);
  assert.deepEqual(JSON.parse(planned.stdout), { written:false, files:f.plan });
  assert.deepEqual(readdirSync(f.root, { recursive:true }), before);
  assert.equal(runCLI(f).status, 0);
  for (const {path:p} of f.plan) assert.equal(lstatSync(p).mode & 0o777, 0o600);
  // A matching regular entry is reused, not rewritten or chmodded, even with
  // non-default whitespace/permissions supplied by its original owner.
  writeFileSync(f.binding, JSON.stringify(f.plan[0].value));
  const fs = runCLI(f, { setup:`fs.chmodSync(destination, 0o640);` });
  assert.equal(fs.status, 0, fs.stderr);
  const saved = f.plan.map(({path:p}) => entryState(p));
  const repeated = runCLI(f);
  assert.equal(repeated.status, 0, repeated.stderr);
  assert.deepEqual(f.plan.map(({path:p}) => entryState(p)), saved);
  assert.equal(lstatSync(f.binding).mode & 0o777, 0o640);
  assert.equal(resolveFrontend(f.front, {}).path, f.binding);
});

for (const kind of ['regular', 'symlink', 'dangling']) test(`exclusive-create failure preserves pre-existing ${kind} temp`, t => {
  const f = fixture(t);
  const target = path.join(f.root, 'temp-target');
  if (kind !== 'dangling') writeFileSync(target, 'belongs to someone else', {mode:0o640});
  const targetBefore = kind !== 'dangling' ? entryState(target) : undefined;
  const result = runCLI(f, { setup:`
    const temp = destination + '.' + process.pid + '.tmp';
    ${kind === 'regular' ? "fs.writeFileSync(temp, 'preexisting temp', {mode:0o640});" : `fs.symlinkSync(${JSON.stringify(target)}, temp);`}
    fs.writeFileSync(${JSON.stringify(path.join(f.root,'temp-before.json'))}, JSON.stringify({ino:fs.lstatSync(temp).ino,mode:fs.lstatSync(temp).mode}));
  ` });
  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stderr, /EEXIST/);
  const temp = `${f.binding}.${result.pid}.tmp`;
  const before = JSON.parse(readFileSync(path.join(f.root,'temp-before.json')));
  assert.equal(lstatSync(temp).ino, before.ino);
  assert.equal(lstatSync(temp).mode, before.mode);
  if (kind === 'regular') assert.equal(readFileSync(temp,'utf8'), 'preexisting temp');
  else assert.equal(readlinkSync(temp), target);
  if (targetBefore) assert.deepEqual(entryState(target), targetBefore); else absent(target);
  for (const {path:p} of f.plan) absent(p);
});

for (const slot of [0,1,2]) for (const kind of ['matching-symlink','dangling-symlink','directory','conflicting-json','invalid-json']) {
  test(`preflight refuses ${kind} at slot ${slot} without touching existing entries`, t => {
    const f = fixture(t), entry = f.plan[slot];
    mkdirSync(path.dirname(entry.path), { recursive:true });
    const target = path.join(f.root, 'symlink-target');
    if (kind === 'matching-symlink') writeFileSync(target, JSON.stringify(entry.value), {mode:0o640});
    if (kind.endsWith('symlink')) symlinkSync(target, entry.path);
    else if (kind === 'directory') mkdirSync(entry.path);
    else writeFileSync(entry.path, kind === 'invalid-json' ? '{broken' : '{"schema_version":99}', {mode:0o640});
    const saved = entryState(entry.path);
    const targetBefore = kind === 'matching-symlink' ? entryState(target) : undefined;
    const result = runCLI(f);
    assert.equal(result.status, 1, result.stdout);
    assert.deepEqual(entryState(entry.path), saved);
    if (targetBefore) assert.deepEqual(entryState(target), targetBefore);
    for (const other of f.plan.filter(x => x !== entry)) absent(other.path);
    assert.equal(readdirSync(f.root, {recursive:true}).some(p => p.endsWith('.tmp')), false);
  });
}

for (const kind of ['regular','dangling-symlink','directory']) test(`publication race cannot replace a newly arrived ${kind}`, t => {
  const f = fixture(t);
  const result = runCLI(f, { setup:`
    for (const name of ['linkSync','renameSync']) {
      const original = fs[name];
      fs[name] = (from, to) => {
        if (to === destination) {
          ${kind === 'regular' ? "fs.writeFileSync(to, 'racing owner', {mode:0o640});" : kind === 'directory' ? 'fs.mkdirSync(to);' : "fs.symlinkSync(destination + '.missing', to);"}
          fs.writeFileSync(${JSON.stringify(path.join(f.root,'race-before.json'))}, JSON.stringify({ino:fs.lstatSync(to).ino, mode:fs.lstatSync(to).mode}));
        }
        return original(from, to);
      };
    }
  ` });
  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stderr, /EEXIST|EISDIR|ENOTEMPTY/);
  const before = JSON.parse(readFileSync(path.join(f.root,'race-before.json')));
  assert.equal(lstatSync(f.binding).ino, before.ino);
  assert.equal(lstatSync(f.binding).mode, before.mode);
  if (kind === 'regular') assert.equal(readFileSync(f.binding,'utf8'), 'racing owner');
  if (kind === 'dangling-symlink') assert.equal(readlinkSync(f.binding), f.binding+'.missing');
  absent(`${f.binding}.${result.pid}.tmp`);
  for (const entry of f.plan.slice(1)) absent(entry.path);
});

for (const phase of ['write','publication']) test(`${phase} failure cleans owned temp only and leaves completed partial enrollment resumable`, t => {
  const f = fixture(t);
  // Preserve an already published regular binding; fault the first locator.
  writeEnrollment([f.plan[0]]);
  const saved = entryState(f.binding);
  const otherTemp = `${f.plan[1].path}.other.tmp`;
  mkdirSync(path.dirname(otherTemp), {recursive:true}); writeFileSync(otherTemp, 'unrelated', {mode:0o640});
  const otherSaved = entryState(otherTemp);
  const result = runCLI(f, { setup:phase === 'write' ? `
    const original = fs.writeFileSync;
    fs.writeFileSync = (file, ...args) => {
      original(file, ...args);
      throw new Error('injected write failure after partial bytes');
    };
  ` : `for (const name of ['linkSync','renameSync']) fs[name] = () => { throw new Error('injected publication failure'); };` });
  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stderr, new RegExp('injected '+phase+' failure'));
  assert.deepEqual(entryState(f.binding), saved);
  assert.deepEqual(entryState(otherTemp), otherSaved);
  absent(`${f.plan[1].path}.${result.pid}.tmp`);
  for (const entry of f.plan.slice(1)) absent(entry.path);
  assert.throws(() => resolveFrontend(f.front, {}), /enrollment unavailable/);
  assert.equal(runCLI(f).status, 0);
  assert.deepEqual(entryState(f.binding), saved);
  assert.deepEqual(entryState(otherTemp), otherSaved);
  assert.equal(resolveFrontend(f.front, {}).path, f.binding);
});

test('ignore policy covers locator/binding and interrupted temp families without hiding unrelated source', t => {
  const f = fixture(t);
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith('GIT_')));
  Object.assign(env, { GIT_CONFIG_NOSYSTEM:'1', GIT_CONFIG_GLOBAL:os.devNull, HOME:f.root, XDG_CONFIG_HOME:f.root });
  const git = args => spawnSync('git', ['-c', `core.excludesFile=${os.devNull}`, ...args], {cwd:f.front, env, encoding:'utf8', timeout:5000});
  assert.equal(git(['init','--quiet']).status, 0);
  writeFileSync(path.join(f.front,'.gitignore'), readFileSync(new URL('../.gitignore',import.meta.url)));
  for (const filename of ['.quantnight/taskctl-selection.json', '.quantnight/taskctl-selection.json.123.tmp',
    '.quantnight/taskctl-binding.json', '.quantnight/taskctl-binding.json.123.tmp']) {
    const result = git(['check-ignore','--no-index',filename]);
    assert.equal(result.status, 0, `${filename}: ${result.stderr}`);
    assert.equal(result.stdout.trim(), filename);
  }
  for (const filename of ['.quantnight/source.js', '.quantnight/other.json', '.quantnight/other.json.123.tmp',
    '.quantnight/taskctl-selection.json.notes', 'src/taskctl-selection.json.123.tmp']) {
    assert.equal(git(['check-ignore','--no-index',filename]).status, 1, filename);
  }
});

test('explicit ignored in-checkout canonical binding location is supported without path discovery', t => {
  const f = fixture(t);
  mkdirSync(path.join(f.front,'.quantnight'));
  f.binding = path.join(f.front,'.quantnight/taskctl-binding.json');
  const before = readdirSync(f.root, {recursive:true});
  const plan = runCLI(f, {write:false});
  assert.equal(plan.status, 0, plan.stderr);
  assert.equal(JSON.parse(plan.stdout).files[0].path, f.binding);
  assert.deepEqual(readdirSync(f.root, {recursive:true}), before);
  const result = runCLI(f);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(resolveFrontend(f.front,{}).path, f.binding);
  assert.equal(resolveFrontend(f.front,{}).binding.backend_root, f.back);
});
