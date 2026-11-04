#!/usr/bin/env node
// Explicit operator setup, never a beforeDev hook or automatic installation.
import { readFileSync } from 'node:fs';
import { parseArgs } from 'node:util';
import { enrollmentPlan, writeEnrollment, upgradeRuntimeBinding, runtimeManagementPlan, writeRuntimeDeployment, runtimeLifecycle } from './dev-registration.mjs';

const HELP = `Usage: node scripts/register-dev-host.mjs [OPTIONS]
Explicit enrollment (default: plan only, no writes/DB/service activation):
  --mode dev|standalone                 Default: dev
  --frontend-root ABS --backend-root ABS
  --executable ABS --binding ABS --target TARGET
  --runtime-executable ABS              Explicit compiled crates/target/debug/taskctl;
                                       schema2 keeps its selector unless explicitly changed
  --runtime-app-config ABS              Optional canonical original AppConfig file;
                                       runtime candidate only, no TOML parsing here
  --expected-original ABS               Exact original binding bytes for explicit upgrade;
                                       schema2 must first be safely stopped/unloaded
  --write                              Publish enrollment and runtime files
  --cutover                            Requires runtime enrollment + --write; bootstrap socket service
Existing runtime management:
  --binding ABS --runtime-action plan|start|stop|reload [--write]
  plan accepts --runtime-executable ABS / --runtime-app-config ABS as candidates;
  without overrides it retains the selected configuration. Mutations require --write.
  Busy stop/reload returns pending without bootout, kill, polling, or retry.
  Start/reload validate original business config before stopping or bootstrapping;
  first socket use activates host. Stop uses registered metadata even if TOML is missing/bad.
  Stop retains enrollment/ready. Asynchronous bootout may return stopRequested/pending;
  a later explicit start reclaims only a native-proven unchanged owned stale socket.
  -h, --help                           Print help before path/config/native/service I/O
No automatic build/install, GUI config discovery, direct-DB fallback, or release fallback.
`;
const argv = process.argv.slice(2);
// Early help is intentionally independent of enrollment and even invalid paths.
// Imports above only declare helpers; no selection/native/filesystem I/O occurs.
if (argv.length === 0 || argv.some(arg => arg === '--help' || arg === '-h')) {
  console.log(HELP);
} else {
  try {
    const { values } = parseArgs({ args:argv, options: {
      mode:{type:'string',default:'dev'}, 'frontend-root':{type:'string'}, 'backend-root':{type:'string'},
      'runtime-executable':{type:'string'}, 'runtime-app-config':{type:'string'}, 'expected-original':{type:'string'},
      'runtime-action':{type:'string'}, cutover:{type:'boolean',default:false},
      executable:{type:'string'}, binding:{type:'string'}, target:{type:'string'}, write:{type:'boolean',default:false},
    } });
    const runtimeExecutable=values['runtime-executable'], runtimeAppConfig=values['runtime-app-config'];
    if (values['runtime-action'] !== undefined) {
      const action=values['runtime-action'];
      if (!['plan','start','stop','reload'].includes(action)) throw new Error('runtime action must be plan/start/stop/reload');
      if (values.cutover || values['expected-original'] !== undefined) throw new Error('runtime action cannot also upgrade/cutover enrollment');
      if (action !== 'plan' && (runtimeAppConfig !== undefined || runtimeExecutable !== undefined)) throw new Error('runtime configuration/executable candidates are only valid for plan or enrollment');
      const runtime=runtimeManagementPlan({bindingPath:values.binding,runtimeExecutable,runtimeAppConfig,metadataOnly:action==='stop'});
      const result=runtimeLifecycle({plan:runtime,action,write:values.write});
      console.log(JSON.stringify({...result,...(action === 'plan' ? {runtime} : {})},null,2));
      if (!result.ok) process.exitCode=1;
    } else {
      if (values.cutover && (!values.write || runtimeExecutable === undefined)) throw new Error('cutover requires explicit runtime enrollment --write');
      const plan = enrollmentPlan({ mode:values.mode,frontendRoot:values['frontend-root'],backendRoot:values['backend-root'],
        executable:values.executable,bindingPath:values.binding,target:values.target,runtimeExecutable,runtimeAppConfig });
      if (values['expected-original'] !== undefined && runtimeExecutable === undefined) throw new Error('expected-original is only for explicit runtime upgrade');
      // Shared Rust owns prospective selection/paths/plist; no Node TOML,
      // fingerprint, socket codec, hash or GUI-default configuration discovery.
      // First-ever setup explicitly writes schema1 before a separate upgrade.
      const runtime = runtimeExecutable === undefined ? undefined : runtimeManagementPlan({
        bindingPath:values.binding,runtimeExecutable,runtimeAppConfig });
      if (runtime) {
        for (const key of ['frontend_root','backend_root','executable','target','profile']) {
          if (runtime.binding[key] !== plan[0].value[key]) throw new Error('shared runtime plan differs from requested binding');
        }
        plan[0].value=runtime.binding;
      }
      if (values.write) {
        if (values['expected-original'] !== undefined) {
          upgradeRuntimeBinding(plan[0], readFileSync(values['expected-original'],'utf8'));
          writeEnrollment(plan.slice(1));
        } else writeEnrollment(plan);
        if (runtime) writeRuntimeDeployment(runtime);
      }
      const lifecycle=values.cutover ? runtimeLifecycle({plan:runtime,action:'start',write:true}) : undefined;
      console.log(JSON.stringify({ written:values.write, files:plan, ...(runtime ? {runtime} : {}), ...(lifecycle ? {lifecycle} : {}) },null,2));
    }
  } catch (e) { console.error(`Explicit enrollment failed: ${e.message}`); process.exitCode=1; }
}
