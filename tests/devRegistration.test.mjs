import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync,mkdirSync,realpathSync,readFileSync,writeFileSync,rmSync, chmodSync, lstatSync, readdirSync, symlinkSync, readlinkSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import os from 'node:os';
import { enrollmentPlan,writeEnrollment,resolveFrontend,LOCATOR,upgradeRuntimeBinding,runtimeManagementPlan,writeRuntimeDeployment,runtimeLifecycle,validateBinding } from '../scripts/dev-registration.mjs';

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

function runtimePlan(f, platform = 'darwin') {
  mkdirSync(path.join(f.front,'crates/target/debug'), {recursive:true});
  return enrollmentPlan({frontendRoot:f.front,backendRoot:f.back,executable:path.join(f.front,'host'),
    bindingPath:f.binding,target:'aarch64-apple-darwin',platform,runtimeExecutable:path.join(f.front,'crates/target/debug/taskctl')});
}

test('schema2 is plan-only until explicit expected-original upgrade; both locators remain the same authority', t => {
  const f = fixture(t); writeEnrollment(f.plan);
  const before = f.plan.map(({path:p}) => entryState(p));
  const plan = runtimePlan(f);
  assert.equal(JSON.parse(readFileSync(f.binding)).schema_version,1);
  assert.equal(plan[0].value.runtime.protocol_version,1);
  assert.throws(()=>writeEnrollment(plan),/conflicting existing enrollment/);
  assert.deepEqual(f.plan.map(({path:p}) => entryState(p)),before);
  const original = readFileSync(f.binding,'utf8');
  assert.throws(()=>upgradeRuntimeBinding(plan[0],original+' '),/expected-original/);
  assert.equal(readFileSync(f.binding,'utf8'),original);
  upgradeRuntimeBinding(plan[0],original);
  assert.equal(resolveFrontend(f.front,{}).binding.schema_version,2);
  assert.deepEqual(f.plan.slice(1).map(({path:p}) => entryState(p)),before.slice(1));
  writeEnrollment(plan); // exact schema2 idempotence, never another config file
});

test('schema2 refuses unsupported platform and an executable from another checkout', t => {
  const f = fixture(t);
  assert.throws(()=>runtimePlan(f,'linux'),/macOS/);
  const plan = runtimePlan(f); plan[0].value.runtime.executable = path.join(f.front,'taskctl');
  writeEnrollment(plan);
  assert.throws(()=>resolveFrontend(f.front,{}),/independent/);
});

for (const fault of ['other-checkout','symlink','busy-lock','changed-original','protocol','configuration','extra']) {
  test(`explicit runtime upgrade rejects ${fault} without taking over binding`, t => {
    const f = fixture(t); writeEnrollment(f.plan);
    const original = readFileSync(f.binding,'utf8');
    const plan = runtimePlan(f);
    if (fault === 'other-checkout') plan[0].value.executable = path.join(f.front,'another-host');
    if (fault === 'symlink') { const target=path.join(f.root,'real-binding'); writeFileSync(target,original);rmSync(f.binding);symlinkSync(target,f.binding); }
    if (fault === 'busy-lock') mkdirSync(f.binding+'.upgrade-lock');
    if (fault === 'changed-original') writeFileSync(f.binding,original+' ');
    if (fault === 'protocol') plan[0].value.runtime.protocol_version = 2;
    if (fault === 'configuration') plan[0].value.runtime.configuration.path = f.front;
    if (fault === 'extra') plan[0].value.runtime.secret = 'not-supported';
    const before = entryState(f.binding);
    assert.throws(()=>upgradeRuntimeBinding(plan[0],original));
    assert.deepEqual(entryState(f.binding),before);
    if (fault === 'busy-lock') assert.ok(lstatSync(f.binding+'.upgrade-lock').isDirectory());
  });
}

function managementFixture(t) {
  const f=fixture(t);writeEnrollment(f.plan);
  const proposed=runtimePlan(f)[0].value;
  const executable=proposed.runtime.executable;
  writeFileSync(executable,'fixture compiled taskctl sentinel');chmodSync(executable,0o700);
  const directory=path.join(f.root,'private-runtime');
  const plan={schema_version:1,command:'runtime-plan',ok:true,business_validated:true,binding:proposed,
    selection:{binding_path:f.binding,backend_root:f.back,frontend_root:f.front,executable,configuration:proposed.runtime.configuration,protocol_version:1},
    paths:{directory,socket:path.join(directory,'task.sock'),ready:path.join(directory,'ready.json'),
      lock:path.join(directory,'host.lock'),label:'fixture.quantnight.runtime',launch_agent:path.join(f.root,'LaunchAgents/fixture.plist')},
    plist:'<?xml version="1.0"?><plist version="1.0"><dict/></plist>'};
  return {...f,executable,plan};
}

test('runtime plan delegates only to explicit compiled taskctl; no filesystem writes or service activation', t => {
  const f=managementFixture(t);const before=readdirSync(f.root,{recursive:true});
  const calls=[];
  const plan=runtimeManagementPlan({bindingPath:f.binding,runtimeExecutable:f.executable,runCommand:(binary,args)=>{
    calls.push({binary,args});return {status:0,stdout:JSON.stringify(f.plan)};
  }});
  assert.deepEqual(calls,[{binary:f.executable,args:['runtime-plan','--binding',f.binding,'--runtime-executable',f.executable]}]);
  assert.deepEqual(plan,f.plan);assert.deepEqual(readdirSync(f.root,{recursive:true}),before);
  assert.equal(JSON.parse(readFileSync(f.binding)).schema_version,1);
});

for (const fault of ['command-failed','invalid-json','different-binding','different-executable','changed-backend']) {
  test(`runtime plan ${fault} cannot alter enrollment or install service`, t => {
    const f=managementFixture(t);const before=readdirSync(f.root,{recursive:true});
    const runCommand=()=>{
      if(fault==='command-failed')return {status:1,stdout:'{"ok":false}'};
      if(fault==='invalid-json')return {status:0,stdout:'not json'};
      const value=structuredClone(f.plan);
      if(fault==='different-binding')value.selection.binding_path=f.binding+'.other';
      if(fault==='different-executable')value.selection.executable=path.join(f.front,'host');
      if(fault==='changed-backend')value.binding.backend_root=f.front;
      return {status:0,stdout:JSON.stringify(value)};
    };
    assert.throws(()=>runtimeManagementPlan({bindingPath:f.binding,runtimeExecutable:f.executable,runCommand}));
    assert.deepEqual(readdirSync(f.root,{recursive:true}),before);
  });
}

test('explicit deployment write only creates private directory/plist from Rust plan, never starts a process', t => {
  const f=managementFixture(t);writeRuntimeDeployment(f.plan);
  assert.equal(lstatSync(f.plan.paths.directory).mode & 0o777,0o700);
  assert.equal(lstatSync(f.plan.paths.launch_agent).mode & 0o777,0o600);
  assert.equal(readFileSync(f.plan.paths.launch_agent,'utf8'),f.plan.plist);
  absent(f.plan.paths.socket);absent(f.plan.paths.ready);absent(f.plan.paths.lock);
  const before=entryState(f.plan.paths.launch_agent);writeRuntimeDeployment(f.plan);
  assert.deepEqual(entryState(f.plan.paths.launch_agent),before);
  assert.equal(JSON.parse(readFileSync(f.binding)).schema_version,1); // caller explicitly owns binding publication
});

for (const fault of ['socket-slot','symlink-directory','wrong-directory-mode','unknown-plist','symlink-plist']) {
  test(`deployment refuses ${fault}; never deletes or takes over unknown paths`, t => {
    const f=managementFixture(t),p=f.plan.paths;
    mkdirSync(p.directory,{mode:0o700});mkdirSync(path.dirname(p.launch_agent));
    if(fault==='socket-slot')writeFileSync(p.socket,'not our socket');
    if(fault==='symlink-directory'){rmSync(p.directory,{recursive:true});symlinkSync(f.back,p.directory);}
    if(fault==='wrong-directory-mode')chmodSync(p.directory,0o755);
    if(fault==='unknown-plist')writeFileSync(p.launch_agent,'another owner',{mode:0o600});
    if(fault==='symlink-plist')symlinkSync(f.binding,p.launch_agent);
    const directory=entryState(p.directory);
    const slot=lstatSync(p.launch_agent,{throwIfNoEntry:false})?entryState(p.launch_agent):undefined;
    assert.throws(()=>writeRuntimeDeployment(f.plan));
    assert.deepEqual(entryState(p.directory),directory);
    if(slot)assert.deepEqual(entryState(p.launch_agent),slot);else absent(p.launch_agent);
    if(fault==='socket-slot')assert.equal(readFileSync(p.socket,'utf8'),'not our socket');
  });
}

function lifecycleFixture(t, fault) {
  const f=managementFixture(t);
  upgradeRuntimeBinding({path:f.binding,value:f.plan.binding},readFileSync(f.binding,'utf8'));
  mkdirSync(f.plan.paths.directory,{mode:0o700});
  const calls=[];let draining=false,unloaded=false,statusCalls=0;
  const identity={selection:f.plan.selection,uid:process.getuid(),owner:{pid:4242,started_seconds:123,started_micros:456},
    generation:'exact-host-generation',ready:true,core_source:'old-managed-source',image:[1,2,3]};
  const runCommand=(binary,args)=>{
    calls.push({binary,args});
    if(binary===f.executable){
      const command=args[0];
      if(args.includes('--stopped-observation'))return {status:0,stdout:JSON.stringify({schemaVersion:1,command:'runtime-status',ok:true,observation:{reclaimable:true,reason:'confirmed_stale',selection:f.plan.selection,identity:structuredClone(identity)}})};
      if(command==='runtime-plan') return {status:0,stdout:JSON.stringify(f.plan)};
      if(command==='runtime-status')statusCalls++;
      if(command==='runtime-drain'){
        assert.deepEqual(args,['runtime-drain','--binding',f.binding,'--generation',identity.generation]);
        draining=true;
      }
      const status={identity:structuredClone(identity),draining,active:fault==='busy' ? 2 : 0};
      if(fault==='generation' && draining)status.identity.generation='another-host';
      if(fault==='birth' && draining)status.identity.owner.started_micros++;
      if(fault==='not-draining' && draining)status.draining=false;
      if(fault==='final-busy' && statusCalls>1)status.active=1;
      if(fault==='selector')status.identity.selection.configuration={kind:'backend_root',path:f.front};
      if(fault==='command-error')return {status:1,stdout:'{"ok":false}'};
      return {status:0,stdout:JSON.stringify({schemaVersion:1,command,ok:true,status})};
    }
    assert.equal(binary,'/bin/launchctl');
    if(args[0]==='print' && fault==='async-bootout' && unloaded)return {status:0,stdout:'service = { pid = 4242 }'};
    if(args[0]==='print')return unloaded ? {status:113,stderr:fault==='unconfirmed-stop'?'permission error':'Could not find service'} :
      {status:0,stdout:`service = {\n pid = ${fault==='launchd-pid'?4243:4242}\n}\n`};
    if(args[0]==='bootout'){
      if(fault==='bootout-error')return {status:1,stderr:'refused'};
      unloaded=true;return {status:0,stdout:''};
    }
    assert.equal(args[0],'bootstrap');
    if(fault==='bootstrap-error')return {status:1,stderr:'refused'};
    return {status:0,stdout:''};
  };
  return {...f,calls,runCommand};
}

test('runtime lifecycle default/plan never issues commands; mutating actions require explicit write', t=>{
  const f=lifecycleFixture(t);
  assert.equal(runtimeLifecycle({plan:f.plan,action:'plan',runCommand:f.runCommand}).ok,true);
  for(const action of ['start','stop','reload'])assert.throws(()=>runtimeLifecycle({plan:f.plan,action,runCommand:f.runCommand}),/explicit --write/);
  assert.deepEqual(f.calls,[]);
});

test('busy drain preserves enrollment, leaves accepted operations alone and never calls launchctl', t=>{
  const f=lifecycleFixture(t,'busy'),before=entryState(f.binding);
  const result=runtimeLifecycle({plan:f.plan,action:'reload',write:true,runCommand:f.runCommand});
  assert.equal(result.ok,false);assert.equal(result.pending,true);assert.equal(result.active,2);
  assert.equal(f.calls.some(c=>c.binary==='/bin/launchctl'),false);
  assert.deepEqual(entryState(f.binding),before);
  absent(path.join(f.plan.paths.directory,'deployment.lock'));
});

test('stale-source same authenticated host drains before exact bootout, stop retains binding and never reconnects/restarts', t=>{
  const f=lifecycleFixture(t),before=entryState(f.binding);
  const result=runtimeLifecycle({plan:f.plan,action:'stop',write:true,runCommand:f.runCommand});
  assert.equal(result.ok,true);assert.equal(result.serviceStopped,true);
  assert.deepEqual(f.calls.map(c=>c.args[0]),['runtime-status','runtime-drain','runtime-status','print','bootout','print','runtime-status']);
  assert.deepEqual(f.calls[4].args,['bootout',`gui/${process.getuid()}/${f.plan.paths.label}`]);
  assert.deepEqual(entryState(f.binding),before);
});

for(const fault of ['generation','birth','not-draining','final-busy','selector','command-error','launchd-pid']){
  test(`safe stop rejects ${fault} before bootout`, t=>{
    const f=lifecycleFixture(t,fault),before=entryState(f.binding);
    assert.throws(()=>runtimeLifecycle({plan:f.plan,action:'reload',write:true,runCommand:f.runCommand}));
    assert.equal(f.calls.some(c=>['bootout','bootstrap'].includes(c.args[0])),false);
    assert.deepEqual(entryState(f.binding),before);
    absent(path.join(f.plan.paths.directory,'deployment.lock'));
  });
}

for(const fault of ['bootout-error','unconfirmed-stop']){
  test(`reload ${fault} cannot bootstrap or fall back`, t=>{
    const f=lifecycleFixture(t,fault);
    assert.throws(()=>runtimeLifecycle({plan:f.plan,action:'reload',write:true,runCommand:f.runCommand}));
    assert.equal(f.calls.some(c=>c.args[0]==='bootstrap'),false);
  });
}

test('start only explicitly bootstraps registered socket plist; reload waits for drained proof first', t=>{
  const f=lifecycleFixture(t);
  const started=runtimeLifecycle({plan:f.plan,action:'start',write:true,runCommand:f.runCommand});
  assert.equal(started.registered,true);assert.equal(started.activated,false);
  assert.deepEqual(f.calls,[{binary:f.executable,args:['runtime-plan','--binding',f.binding]},{binary:'/bin/launchctl',args:['bootstrap',`gui/${process.getuid()}`,f.plan.paths.launch_agent]}]);
  f.calls.length=0;
  const result=runtimeLifecycle({plan:f.plan,action:'reload',write:true,runCommand:f.runCommand});
  assert.equal(result.ok,true);assert.equal(result.previous.serviceStopped,true);
  assert.deepEqual(f.calls.map(c=>c.args[0]),['runtime-plan','runtime-status','runtime-drain','runtime-status','print','bootout','print','runtime-status','bootstrap']);
  absent(f.plan.paths.socket);absent(f.plan.paths.ready);
});

test('reload never unlinks an unknown stale socket slot after authenticated stop', t=>{
  const f=lifecycleFixture(t);writeFileSync(f.plan.paths.socket,'unknown socket ownership');
  assert.throws(()=>runtimeLifecycle({plan:f.plan,action:'reload',write:true,runCommand:f.runCommand}),/socket already exists/);
  assert.equal(readFileSync(f.plan.paths.socket,'utf8'),'unknown socket ownership');
  assert.equal(f.calls.some(c=>c.args[0]==='bootstrap'),false);
});

test('original enrollment CLI runtime plan/start flows use the shared management command runner, never a real service', t=>{
  const f=lifecycleFixture(t);
  const trace=path.join(f.root,'command-trace.json');
  const preload=`import fs from 'node:fs';import cp from 'node:child_process';import {syncBuiltinESMExports} from 'node:module';
    const calls=[];
    cp.spawnSync=(binary,args)=>{
      calls.push({binary,args});fs.writeFileSync(${JSON.stringify(trace)},JSON.stringify(calls));
      if(binary===${JSON.stringify(f.executable)} && args[0]==='runtime-plan')return {status:0,stdout:${JSON.stringify(JSON.stringify(f.plan))}};
      if(binary==='/bin/launchctl' && args[0]==='bootstrap')return {status:0,stdout:''};
      throw new Error('forbidden command outside isolated fixture');
    };syncBuiltinESMExports();`;
  const run=extra=>spawnSync(process.execPath,['--import',`data:text/javascript,${encodeURIComponent(preload)}`,cli,
    '--binding',f.binding,...extra],{encoding:'utf8',timeout:10000,env:{...process.env,NODE_OPTIONS:''}});
  const before=readdirSync(f.root,{recursive:true});
  const plan=run(['--runtime-action','plan']);assert.equal(plan.status,0,plan.stderr);
  assert.equal(JSON.parse(plan.stdout).action,'plan');
  assert.deepEqual(readdirSync(f.root,{recursive:true}).filter(p=>p!=='command-trace.json'),before);
  assert.deepEqual(JSON.parse(readFileSync(trace)).map(c=>c.args[0]),['runtime-plan']);
  const refused=run(['--runtime-action','start']);assert.equal(refused.status,1);
  assert.match(refused.stderr,/explicit --write/);
  assert.deepEqual(JSON.parse(readFileSync(trace)).map(c=>c.args[0]),['runtime-plan']);
  const started=run(['--runtime-action','start','--write']);assert.equal(started.status,0,started.stderr);
  assert.equal(JSON.parse(started.stdout).activated,false);
  assert.deepEqual(JSON.parse(readFileSync(trace)).map(c=>c.args[0]),['runtime-plan','runtime-plan','bootstrap']);
  absent(f.plan.paths.socket);absent(f.plan.paths.ready);
});


for (const argv of [[],['--help'],['-h'],['--binding','/explicit/no-such-qn-help/binding','--write','--cutover','--help'],
  ['--runtime-action','reload','--backend-root','/explicit/no-such-qn-help/backend','-h']]) {
  test(`registration help ${JSON.stringify(argv)} exits before selection/native/writes`, t=>{
    const f=fixture(t),before=readdirSync(f.root,{recursive:true});
    const preload=`import fs from 'node:fs';import cp from 'node:child_process';import {syncBuiltinESMExports} from 'node:module';
      for(const key of ['writeFileSync','mkdirSync','linkSync','renameSync','unlinkSync','rmdirSync','openSync'])fs[key]=()=>{throw new Error('help attempted filesystem mutation/open');};
      for(const key of ['readFileSync','statSync','lstatSync','realpathSync']){
        const original=fs[key];fs[key]=(p,...args)=>{
          if(String(p).startsWith(${JSON.stringify(f.root)}) || String(p).startsWith('/explicit/no-such-qn-help'))throw new Error('help attempted enrollment metadata I/O');
          return original(p,...args);
        };
      }
      cp.spawnSync=()=>{throw new Error('help attempted native/service spawn');};syncBuiltinESMExports();`;
    const result=spawnSync(process.execPath,['--import',`data:text/javascript,${encodeURIComponent(preload)}`,cli,...argv],
      {cwd:f.root,encoding:'utf8',timeout:10000,env:{...process.env,NODE_OPTIONS:''}});
    assert.equal(result.status,0,result.stderr);assert.equal(result.stderr,'');
    for(const flag of ['--runtime-app-config','--expected-original','--runtime-action','--write','--cutover'])assert.ok(result.stdout.includes(flag),flag);
    assert.match(result.stdout,/plan only/);assert.match(result.stdout,/first socket use/);
    assert.deepEqual(readdirSync(f.root,{recursive:true}),before);
  });
}

test('unknown registration options and invalid lifecycle commands still fail, never act as help',t=>{
  const f=fixture(t);
  for(const args of [['--not-supported'],['--runtime-action','unknown','--binding',f.binding]]){
    const result=spawnSync(process.execPath,[cli,...args],{encoding:'utf8',timeout:10000,env:{...process.env,NODE_OPTIONS:''}});
    assert.equal(result.status,1);assert.match(result.stderr,/Explicit enrollment failed/);
    assert.equal(result.stdout,'');
  }
  for(const entry of f.plan)absent(entry.path);
});

function appConfigFixture(t) {
  const f=managementFixture(t),config=path.join(f.root,'original-config.toml');
  // Deliberately not valid TOML: Node may check path metadata, never parse body.
  writeFileSync(config,'not TOML; original Rust loader owns scripts and backend proof');
  const plan=structuredClone(f.plan);
  plan.binding.runtime.configuration={kind:'app_config',path:config};
  plan.selection.configuration={kind:'app_config',path:config};
  return {...f,config,appPlan:plan};
}

test('explicit app_config metadata plan delegates to Rust --app-config without reading TOML or changing locators',t=>{
  const f=appConfigFixture(t),before=f.plan.binding,selected=entryState(f.binding),calls=[];
  const actual=runtimeManagementPlan({bindingPath:f.binding,runtimeExecutable:f.executable,runtimeAppConfig:f.config,
    runCommand:(binary,args)=>{calls.push({binary,args});return {status:0,stdout:JSON.stringify(f.appPlan)};}});
  assert.deepEqual(actual,f.appPlan);
  assert.deepEqual(calls,[{binary:f.executable,args:['runtime-plan','--binding',f.binding,'--runtime-executable',f.executable,'--app-config',f.config]}]);
  assert.deepEqual(entryState(f.binding),selected);assert.deepEqual(f.plan.binding,before);
  const original=readFileSync(f.binding,'utf8');
  upgradeRuntimeBinding({path:f.binding,value:actual.binding},original);
  assert.deepEqual(resolveFrontend(f.front,{}).binding.runtime.configuration,{kind:'app_config',path:f.config});
  for(const root of [f.front,f.back])assert.equal(JSON.parse(readFileSync(path.join(root,LOCATOR))).binding,f.binding);
  calls.length=0;
  const preserved=runtimeManagementPlan({bindingPath:f.binding,runCommand:(binary,args)=>{
    calls.push({binary,args});return {status:0,stdout:JSON.stringify(f.appPlan)};
  }});
  assert.deepEqual(preserved.selection.configuration,f.appPlan.selection.configuration);
  assert.deepEqual(calls,[{binary:f.executable,args:['runtime-plan','--binding',f.binding]}]); // no inferred GUI default
});

for(const fault of ['directory','relative','missing','symlink','unknown-kind','backend-mismatch']) {
  test(`app_config metadata rejects ${fault} without native commands or publication`,t=>{
    const f=appConfigFixture(t),value=structuredClone(f.appPlan.binding);
    if(fault==='directory')value.runtime.configuration.path=f.back;
    if(fault==='relative')value.runtime.configuration.path='relative.toml';
    if(fault==='missing')value.runtime.configuration.path=f.config+'.absent';
    if(fault==='symlink'){const alias=f.config+'.alias';symlinkSync(f.config,alias);value.runtime.configuration.path=alias;}
    if(fault==='unknown-kind')value.runtime.configuration.kind='guess-gui-config';
    if(fault==='backend-mismatch')value.runtime.configuration={kind:'backend_root',path:f.front};
    assert.throws(()=>validateBinding(value,{mode:'setup'}));
    const before=entryState(f.binding);
    assert.throws(()=>upgradeRuntimeBinding({path:f.binding,value},readFileSync(f.binding,'utf8')));
    assert.deepEqual(entryState(f.binding),before);
  });
}

test('original registration CLI forwards runtime-app-config candidates but never infers one or accepts them for mutations',t=>{
  const f=appConfigFixture(t),trace=path.join(f.root,'app-plan-trace.json');
  const preload=`import fs from 'node:fs';import cp from 'node:child_process';import {syncBuiltinESMExports} from 'node:module';
    cp.spawnSync=(binary,args)=>{
      if(binary!==${JSON.stringify(f.executable)} || args[0]!=='runtime-plan')throw new Error('forbidden real native/service command');
      fs.writeFileSync(${JSON.stringify(trace)},JSON.stringify(args));
      return {status:0,stdout:JSON.stringify(args.includes('--app-config')?${JSON.stringify(f.appPlan)}:${JSON.stringify(f.plan)})};
    };
    const original=fs.readFileSync;fs.readFileSync=(p,...args)=>{if(String(p)===${JSON.stringify(f.config)})throw new Error('Node cannot read AppConfig');return original(p,...args);};
    syncBuiltinESMExports();`;
  const run=args=>spawnSync(process.execPath,['--import',`data:text/javascript,${encodeURIComponent(preload)}`,cli,...args],
    {encoding:'utf8',timeout:10000,env:{...process.env,NODE_OPTIONS:''}});
  const base=['--frontend-root',f.front,'--backend-root',f.back,'--executable',path.join(f.front,'host'),'--binding',f.binding,
    '--target','aarch64-apple-darwin','--runtime-executable',f.executable];
  const planned=run([...base,'--runtime-app-config',f.config]);assert.equal(planned.status,0,planned.stderr);
  assert.deepEqual(JSON.parse(planned.stdout).files[0].value,f.appPlan.binding);
  assert.deepEqual(JSON.parse(readFileSync(trace)),['runtime-plan','--binding',f.binding,'--runtime-executable',f.executable,'--app-config',f.config]);
  const backendOnly=run(base);assert.equal(backendOnly.status,0,backendOnly.stderr);
  assert.deepEqual(JSON.parse(backendOnly.stdout).runtime.binding.runtime.configuration,{kind:'backend_root',path:f.back});
  const prospective=run(['--runtime-action','plan','--binding',f.binding,'--runtime-executable',f.executable,'--runtime-app-config',f.config]);
  assert.equal(prospective.status,0,prospective.stderr);assert.deepEqual(JSON.parse(prospective.stdout).runtime.binding,f.appPlan.binding);
  const traceBefore=entryState(trace);
  for(const action of ['start','stop','reload']){
    const rejected=run(['--runtime-action',action,'--binding',f.binding,'--runtime-app-config',f.config,'--write']);
    assert.equal(rejected.status,1);assert.match(rejected.stderr,/only valid for plan or enrollment/);
  }
  assert.deepEqual(entryState(trace),traceBefore);
  assert.equal(JSON.parse(readFileSync(f.binding)).schema_version,1);
  absent(f.appPlan.paths.directory);absent(f.appPlan.paths.launch_agent);
});

test('schema2 executable candidates preserve selected app_config unless explicitly changed',t=>{
  const f=appConfigFixture(t);
  upgradeRuntimeBinding({path:f.binding,value:f.appPlan.binding},readFileSync(f.binding,'utf8'));
  const candidate=enrollmentPlan({frontendRoot:f.front,backendRoot:f.back,executable:path.join(f.front,'host'),
    bindingPath:f.binding,target:'aarch64-apple-darwin',platform:'darwin',runtimeExecutable:f.executable});
  assert.deepEqual(candidate[0].value.runtime.configuration,{kind:'app_config',path:f.config});
  const calls=[];
  runtimeManagementPlan({bindingPath:f.binding,runtimeExecutable:f.executable,runCommand:(binary,args)=>{
    calls.push({binary,args});return {status:0,stdout:JSON.stringify(f.appPlan)};
  }});
  assert.deepEqual(calls,[{binary:f.executable,args:['runtime-plan','--binding',f.binding,'--runtime-executable',f.executable]}]);
  const reset=structuredClone(f.appPlan);reset.binding.runtime.configuration={kind:'backend_root',path:f.back};reset.selection.configuration={kind:'backend_root',path:f.back};
  assert.throws(()=>runtimeManagementPlan({bindingPath:f.binding,runtimeExecutable:f.executable,
    runCommand:()=>({status:0,stdout:JSON.stringify(reset)})}),/configuration mismatch/);
});

function transitionFixture(t) {
  const f=appConfigFixture(t);
  upgradeRuntimeBinding({path:f.binding,value:f.appPlan.binding},readFileSync(f.binding,'utf8'));
  const original=readFileSync(f.binding,'utf8'),nextConfig=path.join(f.root,'next-original-config.toml');
  writeFileSync(nextConfig,'selected file metadata only');
  const next=structuredClone(f.appPlan);
  next.binding.runtime.configuration={kind:'app_config',path:nextConfig};
  next.selection.configuration={kind:'app_config',path:nextConfig};
  // Opaque paths returned by the fake shared Rust plan, never derived by Node.
  const directory=path.join(f.root,'next-runtime');
  next.paths={directory,socket:path.join(directory,'next.sock'),ready:path.join(directory,'ready.json'),
    lock:path.join(directory,'host.lock'),label:'fixture.quantnight.other-selector',launch_agent:path.join(f.root,'LaunchAgents/next.plist')};
  next.plist='next plist from shared runtime-plan';
  mkdirSync(path.dirname(f.appPlan.paths.launch_agent));writeFileSync(f.appPlan.paths.launch_agent,'old plist remains untouched',{mode:0o600});
  return {...f,original,nextConfig,next};
}

for(const state of ['loaded','unknown','socket-slot']){
  test(`schema2 ${state} blocks selector publication before changing binding/plist or touching any job`,t=>{
    const f=transitionFixture(t),calls=[];
    if(state==='socket-slot'){mkdirSync(f.appPlan.paths.directory,{mode:0o700});writeFileSync(f.appPlan.paths.socket,'unknown old socket slot');}
    const binding=entryState(f.binding),plist=entryState(f.appPlan.paths.launch_agent),files=readdirSync(f.root,{recursive:true});
    const runCommand=(binary,args)=>{
      calls.push({binary,args});
      if(binary===f.executable){assert.deepEqual(args,['runtime-plan','--binding',f.binding,'--metadata-only']);return {status:0,stdout:JSON.stringify(f.appPlan)};}
      assert.equal(binary,'/bin/launchctl');assert.deepEqual(args,['print',`gui/${process.getuid()}/${f.appPlan.paths.label}`]);
      return state==='loaded'?{status:0,stdout:'pid = 9999'}:state==='unknown'?{status:1,stderr:'permission denied'}:{status:113,stderr:'Could not find service'};
    };
    assert.throws(()=>upgradeRuntimeBinding({path:f.binding,value:f.next.binding},f.original,{runCommand}),/stop --write|socket ownership/);
    assert.deepEqual(entryState(f.binding),binding);assert.deepEqual(entryState(f.appPlan.paths.launch_agent),plist);
    assert.deepEqual(readdirSync(f.root,{recursive:true}),files);
    assert.deepEqual(calls.map(c=>c.args[0]),['runtime-plan','print']);
    absent(f.next.paths.directory);absent(f.next.paths.launch_agent);
  });
}

test('explicit schema2 replacement after old job is unloaded uses only OLD plan/label then publishes once',t=>{
  const f=transitionFixture(t),calls=[],oldPlist=entryState(f.appPlan.paths.launch_agent);
  const runCommand=(binary,args)=>{
    calls.push({binary,args});
    if(binary===f.executable)return {status:0,stdout:JSON.stringify(f.appPlan)};
    assert.deepEqual(args,['print',`gui/${process.getuid()}/${f.appPlan.paths.label}`]);
    return {status:113,stderr:'Could not find service'};
  };
  upgradeRuntimeBinding({path:f.binding,value:f.next.binding},f.original,{runCommand});
  assert.deepEqual(JSON.parse(readFileSync(f.binding)),f.next.binding);
  assert.deepEqual(entryState(f.appPlan.paths.launch_agent),oldPlist);
  assert.deepEqual(calls.map(c=>c.args[0]),['runtime-plan','print']);
  absent(f.next.paths.launch_agent);absent(f.next.paths.directory);
});

test('original CLI refuses unsafe schema2 selector cutover before replacing pointer, plist, or bootstrapping',t=>{
  const f=transitionFixture(t),expected=path.join(f.root,'expected-original.json'),trace=path.join(f.root,'transition-trace.json');
  writeFileSync(expected,f.original);
  const binding=entryState(f.binding),oldPlist=entryState(f.appPlan.paths.launch_agent);
  const preload=`import fs from 'node:fs';import cp from 'node:child_process';import {syncBuiltinESMExports} from 'node:module';
    const calls=[];cp.spawnSync=(binary,args)=>{
      calls.push({binary,args});fs.writeFileSync(${JSON.stringify(trace)},JSON.stringify(calls));
      if(binary===${JSON.stringify(f.executable)} && args[0]==='runtime-plan')return {status:0,stdout:JSON.stringify(args.includes('--app-config')?${JSON.stringify(f.next)}:${JSON.stringify(f.appPlan)})};
      if(binary==='/bin/launchctl' && args[0]==='print' && args[1]===${JSON.stringify(`gui/${process.getuid()}/${f.appPlan.paths.label}`)})return {status:0,stdout:'pid = 9999'};
      throw new Error('forbidden implicit drain/kill/bootstrap or access to another job');
    };syncBuiltinESMExports();`;
  const result=spawnSync(process.execPath,['--import',`data:text/javascript,${encodeURIComponent(preload)}`,cli,
    '--frontend-root',f.front,'--backend-root',f.back,'--executable',path.join(f.front,'host'),'--target','aarch64-apple-darwin',
    '--binding',f.binding,'--runtime-executable',f.executable,'--runtime-app-config',f.nextConfig,'--expected-original',expected,'--write','--cutover'],
    {encoding:'utf8',timeout:10000,env:{...process.env,NODE_OPTIONS:''}});
  assert.equal(result.status,1);assert.match(result.stderr,/original --binding with --runtime-action stop --write/);
  assert.deepEqual(entryState(f.binding),binding);assert.deepEqual(entryState(f.appPlan.paths.launch_agent),oldPlist);
  assert.deepEqual(JSON.parse(readFileSync(trace)).map(c=>c.args[0]),['runtime-plan','runtime-plan','print']);
  absent(f.next.paths.directory);absent(f.next.paths.launch_agent);
});

test('lifecycle and selector publication share one writer lock; stale plans cannot start either generation',t=>{
  const f=lifecycleFixture(t);mkdirSync(f.binding+'.upgrade-lock');
  assert.throws(()=>runtimeLifecycle({plan:f.plan,action:'start',write:true,runCommand:f.runCommand}),/EEXIST/);
  assert.deepEqual(f.calls,[]);absent(f.plan.paths.launch_agent);
  rmSync(f.binding+'.upgrade-lock',{recursive:true});
  const stale=structuredClone(f.plan);stale.binding.runtime.configuration={kind:'app_config',path:f.binding};
  assert.throws(()=>runtimeLifecycle({plan:stale,action:'start',write:true,runCommand:f.runCommand}),/enrollment changed/);
  assert.deepEqual(f.calls,[]);absent(f.plan.paths.launch_agent);absent(f.binding+'.upgrade-lock');
});

for (const fault of ['missing','malformed']) {
  test(`registered ${fault} AppConfig still permits metadata stop but reload validation precedes drain`,t=>{
    const f=lifecycleFixture(t),config=path.join(f.root,'selected-original.toml');
    writeFileSync(config,'original file metadata; real business validation belongs to Rust');
    const selector={kind:'app_config',path:config};
    f.plan.binding.runtime.configuration=selector;f.plan.selection.configuration=selector;
    writeFileSync(f.binding,JSON.stringify(f.plan.binding));
    if(fault==='missing')rmSync(config);else writeFileSync(config,'[invalid TOML');
    const before=entryState(f.binding),calls=[];
    const runner=(binary,args)=>{
      calls.push({binary,args});
      if(binary===f.executable && args[0]==='runtime-plan'){
        if(!args.includes('--metadata-only'))return {status:1,stdout:JSON.stringify({ok:false,error:'original selected AppConfig invalid'})};
        return {status:0,stdout:JSON.stringify({...f.plan,business_validated:false})};
      }
      return f.runCommand(binary,args);
    };
    const meta=runtimeManagementPlan({bindingPath:f.binding,metadataOnly:true,runCommand:runner});
    assert.equal(meta.business_validated,false);assert.equal(meta.selection.configuration.path,config);
    assert.throws(()=>runtimeLifecycle({plan:meta,action:'reload',write:true,runCommand:runner}),/no install\/bootout\/fallback/);
    assert.deepEqual(calls.map(c=>c.args[0]),['runtime-plan','runtime-plan']);
    assert.equal(f.calls.length,0,'failed prospective validation cannot contact/drain working host');
    const stopped=runtimeLifecycle({plan:meta,action:'stop',write:true,runCommand:runner});
    assert.equal(stopped.serviceStopped,true);assert.equal(stopped.ok,true);
    assert.deepEqual(f.calls.map(c=>c.args[0]),['runtime-status','runtime-drain','runtime-status','print','bootout','print','runtime-status']);
    assert.deepEqual(entryState(f.binding),before);
    absent(f.plan.paths.launch_agent);absent(path.join(f.plan.paths.directory,'deployment.lock'));
  });
}

test('metadata-only plans never authorize deployment writes',t=>{
  const f=managementFixture(t),before=readdirSync(f.root,{recursive:true});
  assert.throws(()=>writeRuntimeDeployment({...f.plan,business_validated:false}),/business configuration must be validated/);
  assert.deepEqual(readdirSync(f.root,{recursive:true}),before);
});


function ownedStaleSocket(f) {
  const run=spawnSync(process.env.QUANTNIGHT_TEST_PYTHON ?? '/usr/bin/python3',['-c','import socket,sys; s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); s.bind(sys.argv[1]); s.close()',f.plan.paths.socket],{encoding:'utf8'});
  assert.equal(run.status,0,run.stderr);chmodSync(f.plan.paths.socket,0o600);writeFileSync(f.plan.paths.ready,'native-owner-fixture',{mode:0o600});
}
function nativeSlot(p) {
  const s=lstatSync(p,{bigint:true});return {dev:s.dev.toString(),ino:s.ino.toString(),uid:Number(s.uid),mode:Number(s.mode&0o777n),ctime_seconds:(s.ctimeNs/1000000000n).toString(),ctime_nanos:(s.ctimeNs%1000000000n).toString()};
}
function observationRunner(f, fault) {
  return (binary,args,options)=>{
    if (binary===f.executable && args.includes('--stopped-observation')) {
      assert.equal(lstatSync(f.binding+'.upgrade-lock').isDirectory(),true);
      assert.equal(lstatSync(path.join(f.plan.paths.directory,'deployment.lock')).isDirectory(),true);
      const observation={reclaimable:!['owner-present','live-listener'].includes(fault),reason:fault==='owner-present'?'owner_present':fault==='live-listener'?'socket_live_or_pending':'confirmed_stale',selection:structuredClone(f.plan.selection),identity:{selection:structuredClone(f.plan.selection),uid:process.getuid(),generation:'old-ready-generation'},socket:nativeSlot(f.plan.paths.socket),ready:nativeSlot(f.plan.paths.ready)};
      if(fault==='wrong-selection')observation.selection.binding_path+='other';
      if(fault==='socket-inode')observation.socket.ino+='0';
      if(fault==='ready-inode')observation.ready.ino+='0';
      if(fault==='bad-proof')observation.reason='caller-said-done';
      if(fault==='changed-inode'){rmSync(f.plan.paths.socket);ownedStaleSocket(f);}
      if(fault==='native-error')return {status:1,stdout:'{"ok":false}'};
      return {status:0,stdout:JSON.stringify({schemaVersion:1,command:'runtime-status',ok:true,observation})};
    }
    if(binary==='/bin/launchctl' && args[0]==='print' && fault!=='loaded')return {status:113,stderr:'Could not find service'};
    return f.runCommand(binary,args,options);
  };
}
test('asynchronous bootout returns pending stopRequested, not stopped/restart, and retains enrollment',t=>{
  const f=lifecycleFixture(t,'async-bootout'),before=entryState(f.binding);
  const result=runtimeLifecycle({plan:f.plan,action:'reload',write:true,runCommand:f.runCommand});
  assert.equal(result.ok,false);assert.equal(result.pending,true);assert.equal(result.stopRequested,true);assert.equal(result.serviceStopped,false);
  assert.equal(f.calls.some(call=>call.args[0]==='bootstrap'),false);assert.deepEqual(entryState(f.binding),before);
});
test('explicit start reclaims only native-confirmed exact owned stale socket under both original locks',t=>{
  const f=lifecycleFixture(t);ownedStaleSocket(f);const ready=entryState(f.plan.paths.ready),binding=entryState(f.binding);
  const result=runtimeLifecycle({plan:f.plan,action:'start',write:true,runCommand:observationRunner(f)});
  assert.equal(result.ok,true);assert.equal(result.registered,true);absent(f.plan.paths.socket);
  assert.deepEqual(entryState(f.plan.paths.ready),ready);assert.deepEqual(entryState(f.binding),binding);
});
for(const fault of ['owner-present','live-listener','wrong-selection','socket-inode','ready-inode','bad-proof','changed-inode','native-error','loaded']) {
  test(`stale start refuses ${fault} without unlink/bootstrap`,t=>{
    const f=lifecycleFixture(t);ownedStaleSocket(f);const ready=entryState(f.plan.paths.ready);
    assert.throws(()=>runtimeLifecycle({plan:f.plan,action:'start',write:true,runCommand:observationRunner(f,fault)}));
    assert.equal(lstatSync(f.plan.paths.socket).isSocket(),true);assert.equal(f.calls.some(call=>call.args[0]==='bootstrap'),false);
    if(fault!=='changed-inode')assert.deepEqual(entryState(f.plan.paths.ready),ready);
  });
}

test('stale cleanup never starts before original business-config validation succeeds',t=>{
  const f=lifecycleFixture(t);ownedStaleSocket(f);const socket=entryState(f.plan.paths.socket),ready=entryState(f.plan.paths.ready);let observed=false;
  assert.throws(()=>runtimeLifecycle({plan:f.plan,action:'start',write:true,runCommand:(binary,args)=>{if(args[0]==='runtime-plan')return {status:1,stdout:'{"ok":false}'};observed=true;throw new Error('must not reach observation or service');}}));
  assert.equal(observed,false);assert.deepEqual(entryState(f.plan.paths.socket),socket);assert.deepEqual(entryState(f.plan.paths.ready),ready);
});

test('unloaded job with native owner still present is pending, never a confirmed stopped/restart',t=>{
  const f=lifecycleFixture(t);const result=runtimeLifecycle({plan:f.plan,action:'stop',write:true,runCommand:(binary,args,options)=>{
    const reply=f.runCommand(binary,args,options);if(args.includes('--stopped-observation')){const value=JSON.parse(reply.stdout);value.observation.reclaimable=false;value.observation.reason='owner_present';return {...reply,stdout:JSON.stringify(value)};}return reply;
  }});assert.equal(result.pending,true);assert.equal(result.stopRequested,true);assert.equal(result.serviceStopped,false);assert.equal(f.calls.some(call=>call.args[0]==='bootstrap'),false);
});
