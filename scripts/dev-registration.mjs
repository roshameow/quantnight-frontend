// Non-secret selection only; never parses GUI/Mongo configuration or searches.
import { lstatSync, statSync, readFileSync, realpathSync, mkdirSync, writeFileSync, openSync, closeSync, fstatSync, constants, linkSync, unlinkSync } from 'node:fs';
import path from 'node:path';
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
  if (!fields(binding, ['schema_version','frontend_root','backend_root','executable','target','profile']) ||
      binding.schema_version !== 1 || binding.profile !== 'debug' || typeof binding.target !== 'string' || !binding.target) throw new Error('invalid dev binding schema/profile/target');
  canonicalDirectory(binding.frontend_root); canonicalDirectory(binding.backend_root);
  if (binding.frontend_root !== root) throw new Error('registered frontend root mismatch');
  if (typeof binding.executable !== 'string' || !path.isAbsolute(binding.executable) || path.normalize(binding.executable) !== binding.executable ||
      realpathSync(path.dirname(binding.executable)) !== path.dirname(binding.executable)) throw new Error('explicit canonical executable parent required');
  const backend = locator(binding.backend_root);
  if (backend.mode !== 'dev' || backend.binding !== selected.binding) throw new Error('frontend/backend locators must reference the same canonical binding');
  return { path: selected.binding, binding };
}

// Explicit one-time setup plan. This accepts supplied paths; original GUI
// AppConfig.env.working_dir is still verified before clients at startup.
export function enrollmentPlan({ frontendRoot, backendRoot, executable, bindingPath, target, mode = 'dev' }) {
  const backend = canonicalDirectory(backendRoot);
  const front = frontendRoot === undefined ? undefined : canonicalDirectory(frontendRoot);
  const selection = mode === 'standalone' ? { schema_version: 1, mode } : { schema_version: 1, mode, binding: bindingPath };
  if (!['dev','standalone'].includes(mode)) throw new Error('mode must be dev or standalone');
  const plan = [];
  if (mode === 'dev') {
    if (!front || typeof target !== 'string' || !target || typeof executable !== 'string' || !path.isAbsolute(executable) ||
        path.normalize(executable) !== executable || realpathSync(path.dirname(executable)) !== path.dirname(executable)) throw new Error('dev enrollment requires explicit canonical frontend/backend/executable and target');
    if (typeof bindingPath !== 'string' || !path.isAbsolute(bindingPath) || path.normalize(bindingPath) !== bindingPath ||
        realpathSync(path.dirname(bindingPath)) !== path.dirname(bindingPath)) throw new Error('binding file needs an explicit canonical existing parent');
    if ([path.join(front,LOCATOR),path.join(backend,LOCATOR)].includes(bindingPath)) throw new Error('binding must not overwrite a locator');
    plan.push({ path: bindingPath, value: { schema_version:1,frontend_root:front,backend_root:backend,executable,target,profile:'debug' } });
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
