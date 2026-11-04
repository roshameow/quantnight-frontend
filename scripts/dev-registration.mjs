// Non-secret selection only; never parses GUI/Mongo configuration or searches.
import { lstatSync, statSync, readFileSync, realpathSync, mkdirSync, writeFileSync, openSync, closeSync, fstatSync, constants, linkSync, unlinkSync, renameSync, rmdirSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
export const LOCATOR = '.quantnight/taskctl-selection.json';
const fields = (v, keys) => v && typeof v === 'object' && !Array.isArray(v) && Object.keys(v).sort().join() === keys.sort().join();
function canonicalDirectory(value) {
  if (typeof value !== 'string' || !path.isAbsolute(value) || realpathSync(value) !== value || !statSync(value).isDirectory()) throw new Error('registration roots must be explicit canonical directories');
  return value;
}
function locator(root) {
  const p = path.join(root, LOCATOR);
  let value;
  try { value = JSON.parse(readFileSync(p, 'utf8')); }
  catch (e) { throw new Error(`taskctl enrollment unavailable at ${p}; explicitly register dev or standalone (${e.message})`); }
  if (value?.schema_version !== 1 || !['dev', 'standalone'].includes(value.mode) ||
      !fields(value, value.mode === 'dev' ? ['schema_version', 'mode', 'binding'] : ['schema_version', 'mode'])) throw new Error(`invalid selection registration: ${p}`);
  if (value.mode === 'dev' && (typeof value.binding !== 'string' || !path.isAbsolute(value.binding) || realpathSync(value.binding) !== value.binding)) throw new Error('registered dev binding must be one canonical existing file');
  return value;
}
export function resolveFrontend(frontendRoot, env = process.env) {
  const root = canonicalDirectory(frontendRoot);
  const selected = locator(root);
  if (selected.mode === 'standalone') {
    if (env.QUANTNIGHT_DEV_BINDING !== undefined) throw new Error('dev environment conflicts with explicit standalone registration');
    return null;
  }
  if (env.QUANTNIGHT_TASKCTL_BIN !== undefined) throw new Error('standalone executable conflicts with registered dev mode');
  if (env.QUANTNIGHT_DEV_BINDING !== undefined && env.QUANTNIGHT_DEV_BINDING !== selected.binding) throw new Error('dev environment conflicts with registered binding');
  const binding = JSON.parse(readFileSync(selected.binding, 'utf8'));
  validateBinding(binding);
  if (binding.frontend_root !== root) throw new Error('registered frontend root mismatch');
  const backend = locator(binding.backend_root);
  if (backend.mode !== 'dev' || backend.binding !== selected.binding) throw new Error('frontend/backend locators must reference the same canonical binding');
  return { path: selected.binding, binding };
}

const BINDING_FIELDS = ['schema_version','frontend_root','backend_root','executable','target','profile'];
function canonicalExecutable(value) {
  if (typeof value !== 'string' || !path.isAbsolute(value) || path.normalize(value) !== value ||
      realpathSync(path.dirname(value)) !== path.dirname(value)) throw new Error('explicit canonical executable parent required');
}
function configurationMetadata(value) {
  if (typeof value !== 'string' || !path.isAbsolute(value) || path.normalize(value) !== value) throw new Error('runtime AppConfig selector must retain its absolute normalized registered path');
  return value;
}
function canonicalConfigurationFile(value) {
  if (typeof value !== 'string' || !path.isAbsolute(value) || realpathSync(value) !== value || !statSync(value).isFile()) throw new Error('runtime AppConfig selector must be an explicit canonical existing file');
  return value;
}
export function validateBinding(binding, {mode='metadata'} = {}) {
  if (!['metadata','setup'].includes(mode)) throw new Error('invalid binding validation mode');
  if (!fields(binding, binding?.schema_version === 2 ? [...BINDING_FIELDS, 'runtime'] : BINDING_FIELDS) ||
      ![1,2].includes(binding.schema_version) || binding.profile !== 'debug' || typeof binding.target !== 'string' || !binding.target) throw new Error('invalid dev binding schema/profile/target');
  canonicalDirectory(binding.frontend_root); canonicalDirectory(binding.backend_root);
  if (binding.schema_version === 1) canonicalExecutable(binding.executable);
  else if (typeof binding.executable !== 'string' || !path.isAbsolute(binding.executable) || path.normalize(binding.executable) !== binding.executable) throw new Error('original GUI selection must remain an absolute normalized path');
  if (binding.schema_version === 2) {
    if (!binding.target.includes('apple-darwin')) throw new Error('runtime enrollment currently requires macOS target');
    const runtime = binding.runtime;
    if (!fields(runtime, ['executable','configuration','protocol_version']) || runtime.protocol_version !== 1 ||
        !fields(runtime.configuration, ['kind','path'])) throw new Error('invalid runtime configuration/protocol');
    if (runtime.configuration.kind === 'app_config') {
      if(mode==='setup') canonicalConfigurationFile(runtime.configuration.path);
      else configurationMetadata(runtime.configuration.path);
    }
    else if (runtime.configuration.kind !== 'backend_root' || runtime.configuration.path !== binding.backend_root) throw new Error('invalid runtime configuration/protocol');
    canonicalExecutable(runtime.executable);
    if (runtime.executable !== path.join(binding.frontend_root, 'crates/target/debug/taskctl')) throw new Error('runtime requires the explicit independent crates/target/debug/taskctl');
  }
  return binding;
}

// Explicit one-time setup plan. This accepts supplied paths; original GUI
// AppConfig.env.working_dir is still verified before clients at startup.
export function enrollmentPlan({ frontendRoot, backendRoot, executable, bindingPath, target, mode = 'dev', runtimeExecutable, runtimeAppConfig, platform = process.platform }) {
  if (runtimeAppConfig !== undefined && runtimeExecutable === undefined) throw new Error('runtime AppConfig candidate requires explicit runtime enrollment executable');
  const backend = canonicalDirectory(backendRoot);
  const front = frontendRoot === undefined ? undefined : canonicalDirectory(frontendRoot);
  const selection = mode === 'standalone' ? { schema_version: 1, mode } : { schema_version: 1, mode, binding: bindingPath };
  if (!['dev','standalone'].includes(mode)) throw new Error('mode must be dev or standalone');
  if (runtimeExecutable !== undefined && (mode !== 'dev' || platform !== 'darwin')) throw new Error('runtime enrollment currently supports macOS dev binding only');
  const plan = [];
  if (mode === 'dev') {
    if (!front || typeof target !== 'string' || !target || typeof executable !== 'string' || !path.isAbsolute(executable) ||
        path.normalize(executable) !== executable || (runtimeExecutable === undefined && realpathSync(path.dirname(executable)) !== path.dirname(executable))) throw new Error('dev enrollment requires explicit canonical frontend/backend/executable and target');
    if (typeof bindingPath !== 'string' || !path.isAbsolute(bindingPath) || path.normalize(bindingPath) !== bindingPath ||
        realpathSync(path.dirname(bindingPath)) !== path.dirname(bindingPath)) throw new Error('binding file needs an explicit canonical existing parent');
    if ([path.join(front,LOCATOR),path.join(backend,LOCATOR)].includes(bindingPath)) throw new Error('binding must not overwrite a locator');
    const value = { schema_version:1,frontend_root:front,backend_root:backend,executable,target,profile:'debug' };
    if (runtimeExecutable !== undefined) {
      value.schema_version = 2;
      let configuration={kind:'backend_root',path:backend};
      // Only schema1/new enrollment defaults to backend_root. Existing schema2
      // metadata is retained unless the operator explicitly changes selector.
      const existing=lstatSync(bindingPath,{throwIfNoEntry:false});
      if (existing) {
        if (!existing.isFile() || realpathSync(bindingPath) !== bindingPath) throw new Error('runtime candidate binding must be canonical and regular');
        const fd=openSync(bindingPath,constants.O_RDONLY|constants.O_NOFOLLOW);
        try {
          const opened=fstatSync(fd);
          if (opened.dev !== existing.dev || opened.ino !== existing.ino) throw new Error('runtime candidate binding changed');
          const previous=validateBinding(JSON.parse(readFileSync(fd,'utf8')));
          if (previous.runtime) {
            for (const key of ['frontend_root','backend_root','executable','target','profile']) if (previous[key] !== value[key]) throw new Error('runtime candidate cannot replace another checkout/GUI binding');
            configuration=previous.runtime.configuration;
          }
        } finally { closeSync(fd); }
      }
      if (runtimeAppConfig !== undefined) configuration={kind:'app_config',path:canonicalConfigurationFile(runtimeAppConfig)};
      value.runtime = { executable:runtimeExecutable, configuration, protocol_version:1 };
    }
    validateBinding(value,{mode:'setup'});
    plan.push({ path: bindingPath, value });
  }
  if (front) plan.push({ path:path.join(front,LOCATOR), value:selection });
  plan.push({ path:path.join(backend,LOCATOR), value:selection });
  if (new Set(plan.map(x => x.path)).size !== plan.length) throw new Error('enrollment paths must be distinct');
  return plan;
}
// lstat sees dangling links too. Read only a regular no-follow descriptor;
// idempotence must not chmod, rewrite, or silently accept a symlink slot.
function existingEnrollment(entry) {
  const current = lstatSync(entry.path, { throwIfNoEntry:false });
  if (!current) return false;
  if (!current.isFile()) throw new Error(`enrollment slot must be a regular file: ${entry.path}`);
  const fd = openSync(entry.path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const opened = fstatSync(fd);
    if (!opened.isFile() || opened.dev !== current.dev || opened.ino !== current.ino) throw new Error(`enrollment slot changed: ${entry.path}`);
    if (JSON.stringify(JSON.parse(readFileSync(fd, 'utf8'))) !== JSON.stringify(entry.value)) throw new Error(`conflicting existing enrollment: ${entry.path}; resolve explicitly, not by environment override`);
  } finally { closeSync(fd); }
  return true;
}
export function writeEnrollment(plan) {
  // Refuse every pre-existing conflict before writing any slot. A later race
  // may leave partial enrollment; clients fail closed and matching files allow
  // an explicit retry. There is deliberately no all-files transaction.
  for (const entry of plan) existingEnrollment(entry);
  for (const entry of plan) {
    if (existingEnrollment(entry)) continue;
    mkdirSync(path.dirname(entry.path), { recursive:true });
    if (realpathSync(path.dirname(entry.path)) !== path.dirname(entry.path)) throw new Error('enrollment parent changed/symlinked');
    const temp = `${entry.path}.${process.pid}.tmp`;
    // Open outside the cleanup scope: an exclusive-create failure does NOT
    // confer ownership of a pre-existing temp (including dangling symlinks).
    const fd = openSync(temp, 'wx', 0o600);
    let owned;
    try {
      owned = fstatSync(fd);
      writeFileSync(fd, JSON.stringify(entry.value,null,2)+'\n');
      // Same-parent hard-link publication is atomic and no-replace. Unlike
      // rename, it refuses any destination that arrived after preflight.
      linkSync(temp, entry.path);
    } finally {
      try { closeSync(fd); }
      finally {
        const remaining = lstatSync(temp, { throwIfNoEntry:false });
        if (owned && remaining?.dev === owned.dev && remaining?.ino === owned.ino) unlinkSync(temp);
      }
    }
  }
}

// Explicit expected-original upgrade of this one existing binding only. The
// locators and GUI identity are unchanged. A private exclusive lock serializes
// cooperating upgrade writers; never remove a pre-existing/stale lock.
export function upgradeRuntimeBinding(entry, expectedOriginal, {runCommand=spawnSync} = {}) {
  validateBinding(entry.value,{mode:'setup'});
  if (entry.value.schema_version !== 2 || typeof expectedOriginal !== 'string') throw new Error('runtime upgrade requires explicit expected-original bytes');
  const previous = validateBinding(JSON.parse(expectedOriginal));
  const originalBase = { ...previous }; delete originalBase.runtime; originalBase.schema_version = 1;
  const newBase = { ...entry.value }; delete newBase.runtime; newBase.schema_version = 1;
  for (const key of BINDING_FIELDS) if (originalBase[key] !== newBase[key]) throw new Error('runtime upgrade cannot replace another checkout/GUI binding');
  if (realpathSync(entry.path) !== entry.path) throw new Error('upgrade binding must be canonical, not symlinked');
  const selected = resolveFrontend(entry.value.frontend_root, {});
  if (!selected || selected.path !== entry.path) throw new Error('upgrade requires both existing locators to select this binding');
  const lock = entry.path + '.upgrade-lock';
  mkdirSync(lock, { mode:0o700 }); // EEXIST never confers ownership.
  let fd, tempFd, tempOwned;
  const temp = `${entry.path}.${process.pid}.tmp`;
  try {
    const current = lstatSync(entry.path);
    if (!current.isFile() || current.uid !== process.getuid()) throw new Error('upgrade binding must be an owned regular file');
    fd = openSync(entry.path, constants.O_RDONLY | constants.O_NOFOLLOW);
    const opened = fstatSync(fd);
    if (current.dev !== opened.dev || current.ino !== opened.ino || readFileSync(fd, 'utf8') !== expectedOriginal) throw new Error('expected-original binding conflict');
    if (previous.runtime) assertRuntimeUnloaded(entry.path,runCommand);
    tempFd = openSync(temp, 'wx', 0o600); tempOwned = fstatSync(tempFd);
    writeFileSync(tempFd, JSON.stringify(entry.value,null,2)+'\n');
    // Revalidate immediately before atomic publication; no unlink of unknown
    // files. This is expected-original under the registration writer lock, not
    // an OS CAS against malicious non-cooperating filesystem writers.
    const final = lstatSync(entry.path);
    if (final.dev !== opened.dev || final.ino !== opened.ino || readFileSync(entry.path,'utf8') !== expectedOriginal) throw new Error('expected-original binding changed');
    renameSync(temp, entry.path);
  } finally {
    if (fd !== undefined) closeSync(fd);
    if (tempFd !== undefined) closeSync(tempFd);
    const remaining = lstatSync(temp, {throwIfNoEntry:false});
    if (tempOwned && remaining?.dev === tempOwned.dev && remaining?.ino === tempOwned.ino) unlinkSync(temp);
    rmdirSync(lock);
  }
}

// The compiled shared owner is the ONLY runtime path/plist/IPC authority.
// runCommand is the original command-runner seam for isolated fixtures; no
// socket codec, peer check, path hash, Mongo config, or build lives in Node.
function runJSON(executable, args, runCommand) {
  const result = runCommand(executable,args,{encoding:'utf8',stdio:['ignore','pipe','pipe']});
  if (result.error || result.status !== 0) throw new Error('shared runtime management command failed; no install/bootout/fallback performed');
  let value;
  try { value=JSON.parse(result.stdout); } catch { throw new Error('shared runtime management returned invalid JSON'); }
  if (!value || typeof value !== 'object' || value.ok === false) throw new Error('shared runtime management refused operation');
  return value;
}
function managementExecutable(binding, executable) {
  const selected = executable ?? binding.runtime?.executable;
  canonicalExecutable(selected);
  if (selected !== path.join(binding.frontend_root,'crates/target/debug/taskctl')) throw new Error('runtime requires explicit independent crates/target/debug/taskctl');
  const info = lstatSync(selected);
  if (!info.isFile() || !(info.mode & 0o111) || realpathSync(selected) !== selected) throw new Error('explicit compiled runtime executable is unavailable; no build/install fallback');
  return selected;
}
export function runtimeManagementPlan({bindingPath,runtimeExecutable,runtimeAppConfig,metadataOnly=false,runCommand=spawnSync}) {
  if (realpathSync(bindingPath) !== bindingPath || !lstatSync(bindingPath).isFile()) throw new Error('runtime binding must be one canonical regular file');
  const original = validateBinding(JSON.parse(readFileSync(bindingPath,'utf8')));
  const executable = managementExecutable(original,runtimeExecutable);
  const args = ['runtime-plan','--binding',bindingPath];
  if (runtimeExecutable !== undefined) args.push('--runtime-executable',executable);
  if (runtimeAppConfig !== undefined) args.push('--app-config',canonicalConfigurationFile(runtimeAppConfig));
  if (metadataOnly) args.push('--metadata-only');
  const plan = runJSON(executable,args,runCommand);
  if (plan.schema_version !== 1 || plan.command !== 'runtime-plan' || plan.ok !== true ||
      plan.selection?.binding_path !== bindingPath || plan.selection?.executable !== executable ||
      plan.selection?.backend_root !== original.backend_root || typeof plan.plist !== 'string' || (!metadataOnly && plan.business_validated !== true)) throw new Error('shared runtime plan selection mismatch');
  validateBinding(plan.binding);
  for (const key of BINDING_FIELDS.filter(k=>k!=='schema_version')) if (plan.binding[key] !== original[key]) throw new Error('shared runtime plan changed the original binding');
  if (plan.binding.runtime?.executable !== executable) throw new Error('shared runtime plan executable mismatch');
  const expectedConfiguration = runtimeAppConfig !== undefined ? {kind:'app_config',path:runtimeAppConfig} :
    original.runtime?.configuration ?? {kind:'backend_root',path:original.backend_root};
  for (const configuration of [plan.binding.runtime.configuration,plan.selection.configuration]) {
    if (!fields(configuration,['kind','path']) || configuration.kind !== expectedConfiguration?.kind || configuration.path !== expectedConfiguration.path) throw new Error('shared runtime plan configuration mismatch');
  }
  for (const key of ['directory','socket','ready','lock','launch_agent']) {
    const value = plan.paths?.[key];
    if (typeof value !== 'string' || !path.isAbsolute(value) || path.normalize(value) !== value) throw new Error('shared runtime plan has invalid paths');
  }
  if (typeof plan.paths.label !== 'string' || !/^[A-Za-z0-9.-]+$/.test(plan.paths.label)) throw new Error('shared runtime plan has invalid label');
  return plan;
}

// Schema2 re-enrollment must not publish a new selector/path before the old
// job is safely stopped. Query only the OLD shared plan/label; never connect
// (which could activate), implicitly drain/kill, or inspect another job.
function assertRuntimeUnloaded(bindingPath,runCommand) {
  const old=runtimeManagementPlan({bindingPath,metadataOnly:true,runCommand});
  const target=`gui/${process.getuid()}/${old.paths.label}`;
  const loaded=runCommand('/bin/launchctl',['print',target],{encoding:'utf8',stdio:['ignore','pipe','pipe']});
  if (loaded.error || loaded.status === 0 || !/Could not find service/i.test(loaded.stderr ?? '')) {
    throw new Error('existing schema2 runtime may still be loaded; first use the original --binding with --runtime-action stop --write before re-enrollment');
  }
  if (lstatSync(old.paths.socket,{throwIfNoEntry:false})) throw new Error('old runtime socket ownership remains unresolved after stop; preserve binding and resolve explicitly before re-enrollment');
}

// Never chmod/take over unknown directories (including symlink ancestors).
function ownedDirectory(directory, privateLeaf = false, requireOwner = false) {
  const parent=path.dirname(directory);
  if (parent !== directory) ownedDirectory(parent);
  let info=lstatSync(directory,{throwIfNoEntry:false});
  if (!info) { mkdirSync(directory,{mode:0o700}); info=lstatSync(directory); }
  if (!info.isDirectory() || (requireOwner && info.uid !== process.getuid()) || (privateLeaf && (info.uid !== process.getuid() || (info.mode & 0o777) !== 0o700))) throw new Error('runtime directory owner/mode/symlink conflict');
  // System-owned ancestors are fine; user-writable-by-others parents are not.
  if ((info.mode & 0o022) && !(info.mode & 0o1000)) throw new Error('runtime directory has unsafe writable ancestors');
}
function writeRuntimePlist(plan) {
  ownedDirectory(plan.paths.directory,true);
  // launchd owns the socket. Never unlink/reuse an unknown/stale slot; safe
  // reload must bootout first and leave no conflicting slot behind.
  if (lstatSync(plan.paths.socket,{throwIfNoEntry:false})) throw new Error('runtime socket already exists; use authenticated drain/stop before explicit adoption');
  const destination=plan.paths.launch_agent;
  ownedDirectory(path.dirname(destination),false,true);
  const current=lstatSync(destination,{throwIfNoEntry:false});
  if (current) {
    if (!current.isFile() || current.uid !== process.getuid() || (current.mode & 0o777) !== 0o600) throw new Error('LaunchAgent owner/mode/symlink conflict');
    const fd=openSync(destination,constants.O_RDONLY|constants.O_NOFOLLOW);
    try {
      const opened=fstatSync(fd);
      if (opened.dev !== current.dev || opened.ino !== current.ino || readFileSync(fd,'utf8') !== plan.plist) throw new Error('conflicting existing LaunchAgent; no overwrite');
    } finally { closeSync(fd); }
    return;
  }
  const temp=`${destination}.${process.pid}.tmp`;
  const fd=openSync(temp,'wx',0o600); let owned;
  try { owned=fstatSync(fd);writeFileSync(fd,plan.plist);linkSync(temp,destination); }
  finally {
    closeSync(fd);
    const remaining=lstatSync(temp,{throwIfNoEntry:false});
    if (owned && remaining?.dev===owned.dev && remaining?.ino===owned.ino) unlinkSync(temp);
  }
}
export function writeRuntimeDeployment(plan) {
  if (plan.business_validated !== true) throw new Error('business configuration must be validated by original taskctl before deployment');
  // Only called after an explicit operator write. No launchctl here, no DB.
  writeRuntimePlist(plan);
}

// Management stdout uses the original taskctl runtime_entry.rs envelope.
// Authentication/PID-birth checks remain in the shared Rust management client.
function hostStatus(plan, command, generation, runCommand) {
  const args=[command,'--binding',plan.selection.binding_path];
  if (generation !== undefined) args.push('--generation',generation);
  const value=runJSON(plan.selection.executable,args,runCommand);
  const status=value.status, identity=status?.identity, owner=identity?.owner;
  if (value.schemaVersion !== 1 || value.command !== command || value.ok !== true ||
      !Number.isSafeInteger(status?.active) || status.active < 0 || typeof status.draining !== 'boolean' ||
      !identity || typeof identity.generation !== 'string' || !identity.generation ||
      identity.uid !== process.getuid() || identity.selection?.binding_path !== plan.selection.binding_path ||
      identity.selection?.backend_root !== plan.selection.backend_root ||
      JSON.stringify(identity.selection?.configuration) !== JSON.stringify(plan.selection.configuration) ||
      identity.selection?.protocol_version !== plan.selection.protocol_version ||
      !Number.isSafeInteger(owner?.pid) || owner.pid < 2 || !Number.isSafeInteger(owner.started_seconds) ||
      !Number.isSafeInteger(owner.started_micros)) throw new Error('shared runtime management status identity mismatch');
  if (generation !== undefined && identity.generation !== generation) throw new Error('runtime host generation changed; no bootout');
  return status;
}
function launchctl(args, runCommand) {
  const value=runCommand('/bin/launchctl',args,{encoding:'utf8',stdio:['ignore','pipe','pipe']});
  if (value.error || value.status !== 0) throw new Error('explicit launchctl operation failed; no forced kill or fallback');
  return value;
}
function stopRuntime(plan, runCommand) {
  const initial=hostStatus(plan,'runtime-status',undefined,runCommand);
  const generation=initial.identity.generation;
  const drained=hostStatus(plan,'runtime-drain',generation,runCommand);
  if (JSON.stringify(drained.identity.owner) !== JSON.stringify(initial.identity.owner) || !drained.draining) throw new Error('runtime drain owner changed/refused; no bootout');
  if (drained.active !== 0) return {ok:false,pending:true,serviceStopped:false,generation,active:drained.active};
  // No polling or busy-kill. One authoritative final observation proves that
  // this exact draining generation still has no unfinished core futures.
  const final=hostStatus(plan,'runtime-status',undefined,runCommand);
  if (final.identity.generation !== generation || JSON.stringify(final.identity.owner) !== JSON.stringify(initial.identity.owner) ||
      !final.draining || final.active !== 0) throw new Error('runtime drain not settled for the exact host; no bootout');
  const target=`gui/${process.getuid()}/${plan.paths.label}`;
  const loaded=launchctl(['print',target],runCommand);
  const pids=[...(loaded.stdout ?? '').matchAll(/^\s*pid = ([0-9]+)\s*$/gm)];
  if (pids.length !== 1 || Number(pids[0][1]) !== final.identity.owner.pid) throw new Error('loaded launchd service PID differs from authenticated drained host; no bootout');
  launchctl(['bootout',target],runCommand);
  // Do not reconnect (would activate), delete the socket, erase ready evidence,
  // or change enrollment. An absent/failed stop remains an explicit failure.
  const unloaded=runCommand('/bin/launchctl',['print',target],{encoding:'utf8',stdio:['ignore','pipe','pipe']});
  if (unloaded.error || (unloaded.status !== 0 && !/Could not find service/i.test(unloaded.stderr ?? ''))) throw new Error('launchd stop confirmation unavailable; enrollment retained, no restart');
  if (unloaded.status === 0) return {ok:false,pending:true,stopRequested:true,serviceStopped:false,generation,active:0,owner:final.identity.owner};
  const observed=stoppedObservation(plan,runCommand);
  if (observed.identity.generation !== generation || JSON.stringify(observed.identity.owner) !== JSON.stringify(final.identity.owner)) throw new Error('stopped ready owner/generation changed; no restart');
  if (!observed.reclaimable) return {ok:false,pending:true,stopRequested:true,serviceStopped:false,generation,active:0,owner:final.identity.owner};
  return {ok:true,pending:false,stopRequested:true,serviceStopped:true,generation,active:0,owner:final.identity.owner};
}
// Native owner alone interprets ready/process/socket facts. This metadata
// observation is not a live authenticated host or a business terminal receipt.
function stoppedObservation(plan,runCommand) {
  const value=runJSON(plan.selection.executable,['runtime-status','--binding',plan.selection.binding_path,'--stopped-observation'],runCommand);
  const observation=value.observation;
  if (value.schemaVersion !== 1 || value.command !== 'runtime-status' || !observation ||
      JSON.stringify(observation.selection) !== JSON.stringify(plan.selection) ||
      observation.identity?.uid !== process.getuid() || !observation.identity?.generation ||
      JSON.stringify(observation.identity?.selection) !== JSON.stringify(plan.selection)) throw new Error('native stopped observation selection mismatch; no reclaim');
  if (typeof observation.reclaimable !== 'boolean' || (observation.reclaimable && observation.reason !== 'confirmed_stale')) throw new Error('native stopped observation is not a validated stale proof');
  return observation;
}
function reclaimSelectedStaleSocket(plan,runCommand) {
  const slot=lstatSync(plan.paths.socket,{throwIfNoEntry:false,bigint:true});
  if (!slot) return true;
  if (!slot.isSocket() || slot.uid !== BigInt(process.getuid()) || (slot.mode & 0o777n) !== 0o600n) throw new Error('runtime socket already exists with unknown type/owner/mode; no reclaim');
  const observation=stoppedObservation(plan,runCommand);
  if (observation.reclaimable !== true) return false;
  const matches=(file,snapshot,socket)=>{
    const info=lstatSync(file,{throwIfNoEntry:false,bigint:true});
    if (!info || !(socket ? info.isSocket() : info.isFile()) || !snapshot ||
        typeof snapshot.dev !== 'string' || typeof snapshot.ino !== 'string' ||
        info.dev.toString() !== snapshot.dev || info.ino.toString() !== snapshot.ino ||
        info.uid !== BigInt(process.getuid()) || snapshot.uid !== process.getuid() ||
        (info.mode & 0o777n) !== 0o600n || snapshot.mode !== 0o600 ||
        (info.ctimeNs / 1000000000n).toString() !== snapshot.ctime_seconds ||
        (info.ctimeNs % 1000000000n).toString() !== snapshot.ctime_nanos) throw new Error('selected socket/ready inode or owner/mode changed; no reclaim');
    return info;
  };
  const target=`gui/${process.getuid()}/${plan.paths.label}`;
  const unloaded=runCommand('/bin/launchctl',['print',target],{encoding:'utf8',stdio:['ignore','pipe','pipe']});
  if (unloaded.error || unloaded.status === 0 || !/Could not find service/i.test(unloaded.stderr ?? '')) throw new Error('selected job changed/not unloaded; no reclaim');
  matches(plan.paths.ready,observation.ready,false);
  const current=matches(plan.paths.socket,observation.socket,true);
  if (current.dev !== slot.dev || current.ino !== slot.ino) throw new Error('selected socket changed since lifecycle entry; no reclaim');
  // Cooperative binding+deployment locks cover this final check/unlink and
  // bootstrap. This is NOT an atomic CAS against malicious same-UID writers.
  unlinkSync(plan.paths.socket);
  return true;
}
export function runtimeLifecycle({plan,action,write=false,runCommand=spawnSync}) {
  if (!['plan','start','stop','reload'].includes(action)) throw new Error('runtime action must be plan/start/stop/reload');
  if (action === 'plan') return {ok:true,action,paths:plan.paths};
  if (!write) throw new Error('runtime lifecycle requires explicit --write; default plan never activates');
  // Share the enrollment writer lock: an explicit start cannot load the old
  // job between the unloaded proof and replacement of the single binding.
  const bindingLock=plan.selection.binding_path+'.upgrade-lock';
  mkdirSync(bindingLock,{mode:0o700});
  const bindingOwned=lstatSync(bindingLock);
  try { return runtimeLifecycleLocked(plan,action,runCommand); }
  finally {
    const remaining=lstatSync(bindingLock,{throwIfNoEntry:false});
    if (!remaining || remaining.dev !== bindingOwned.dev || remaining.ino !== bindingOwned.ino) throw new Error('runtime binding lock changed; do not reclaim');
    rmdirSync(bindingLock);
  }
}
function runtimeLifecycleLocked(plan,action,runCommand) {
  const current=validateBinding(JSON.parse(readFileSync(plan.selection.binding_path,'utf8')));
  for (const key of BINDING_FIELDS) if (current[key] !== plan.binding[key]) throw new Error('runtime enrollment changed since plan; re-plan before lifecycle action');
  for (const key of ['executable','protocol_version']) if (current.runtime?.[key] !== plan.binding.runtime?.[key]) throw new Error('runtime enrollment changed since plan; re-plan before lifecycle action');
  for (const key of ['kind','path']) if (current.runtime?.configuration[key] !== plan.binding.runtime?.configuration[key]) throw new Error('runtime enrollment changed since plan; re-plan before lifecycle action');
  // Validate prospective business config through the original Rust owner
  // BEFORE any working host is drained/stopped. Stop is metadata-only.
  if (action === 'start' || action === 'reload') {
    plan=runtimeManagementPlan({bindingPath:plan.selection.binding_path,runCommand});
  }
  if (action === 'start') ownedDirectory(plan.paths.directory,true);
  const directory=lstatSync(plan.paths.directory);
  if (!directory.isDirectory() || directory.uid !== process.getuid() || (directory.mode & 0o777) !== 0o700) throw new Error('runtime lifecycle directory owner/mode conflict');
  const lock=path.join(plan.paths.directory,'deployment.lock');
  mkdirSync(lock,{mode:0o700}); // Never reclaim an unknown/stale lock.
  const owned=lstatSync(lock);
  try {
    let stopped;
    if (action !== 'start') {
      stopped=stopRuntime(plan,runCommand);
      if (!stopped.ok || action === 'stop') return {...stopped,action};
    }
    if (!reclaimSelectedStaleSocket(plan,runCommand)) {
      if (stopped?.stopRequested) return {ok:false,pending:true,stopRequested:true,serviceStopped:false,action,generation:stopped.generation,active:0};
      throw new Error('selected runtime owner/socket still present or uncertain; no reclaim/start');
    }
    writeRuntimeDeployment(plan);
    launchctl(['bootstrap',`gui/${process.getuid()}`,plan.paths.launch_agent],runCommand);
    // Registration only: no kickstart/RunAtLoad/GUI launch, first socket use
    // will activate the original taskctl host through launchd.
    return {ok:true,action,serviceStopped:false,registered:true,activated:false,...(stopped ? {previous:stopped} : {})};
  } finally {
    const remaining=lstatSync(lock,{throwIfNoEntry:false});
    if (!remaining || remaining.dev !== owned.dev || remaining.ino !== owned.ino) throw new Error('runtime deployment lock changed; do not reclaim');
    rmdirSync(lock);
  }
}
