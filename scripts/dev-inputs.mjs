// Trigger only. Rust owns fingerprints/admission; Tauri owns kill/wait/build/restart.
import { realpathSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { resolveFrontend } from './dev-registration.mjs';

export function relevant(relative) {
  const p = relative.split(path.sep).join('/');
  if (p.split('/').some(x => ['target', 'node_modules', 'artifacts', 'logs', '.quantnight', '.quantnight-dev'].includes(x))) return false;
  if (/^crates\/(task-core|taskctl)\/src\/.+\.rs$/.test(p)) return true;
  return new Set([
    'crates/Cargo.toml', 'crates/Cargo.lock', 'crates/task-core/Cargo.toml', 'crates/taskctl/Cargo.toml',
    'crates/source_identity.rs', 'crates/task-core/build.rs', 'crates/taskctl/build.rs',
    'src-tauri/Cargo.lock', 'scripts/dev-registration.mjs', 'scripts/register-dev-host.mjs',
    'vite.config.js', 'scripts/dev-inputs.mjs', 'package.json', 'package-lock.json', '.cargo/config.toml',
  ]).has(p);
}

export function devInputs(frontendRoot, env = process.env) {
  return {
    name: 'quantnight-dev-rust-inputs',
    apply: 'serve',
    configureServer(server) {
      const frontend = realpathSync(frontendRoot);
      const selected = resolveFrontend(frontend, env);
      if (!selected) return;
      // Under existing src/ recursive watch. .taurignore explicitly admits this
      // file; Vite ignores src-tauri, so our output cannot feed itself.
      const stamp = path.join(frontend, 'src-tauri/src/dev-input.stamp');
      let serial = 0;
      let timer;
      const flush = () => {
        timer = undefined;
        writeFileSync(stamp, `dev-input ${process.pid} ${Date.now()} ${++serial}\n`);
      };
      const onEvent = (event, filename) => {
        if (!['add', 'change', 'unlink'].includes(event) || !relevant(path.relative(frontend, path.resolve(filename)))) return;
        clearTimeout(timer);
        timer = setTimeout(flush, 80);
      };
      // Attach first, then add roots: avoid a gap within this existing watcher.
      // A Tauri startup watch gap remains fail-closed via runtime fingerprinting.
      server.watcher.on('all', onEvent);
      server.watcher.add([
        path.join(frontend, 'crates/task-core/src'), path.join(frontend, 'crates/taskctl/src'),
        ...['crates/Cargo.toml', 'crates/Cargo.lock', 'crates/task-core/Cargo.toml', 'crates/taskctl/Cargo.toml',
    'crates/source_identity.rs', 'crates/task-core/build.rs', 'crates/taskctl/build.rs',
    'src-tauri/Cargo.lock', 'scripts/dev-registration.mjs', 'scripts/register-dev-host.mjs',
          'scripts/dev-inputs.mjs', 'package.json', 'package-lock.json', '.cargo'].map(p => path.join(frontend, p)),
      ]);
      flush();
      server.httpServer?.once('close', () => { clearTimeout(timer); server.watcher.off('all', onEvent); });
    },
  };
}

// Vite/chokidar must traverse only the src-tauri parent to reach this exact
// lockfile. Every other child (including src/, gen/, target/) stays ignored.
// The old blanket glob would prevent an explicit watcher.add(lock) from working.
export function tauriInputIgnore(frontendRoot) {
  const root = path.join(frontendRoot, 'src-tauri');
  return filename => {
    const relative = path.relative(root, path.resolve(filename));
    if (relative === '..' || relative.startsWith('..'+path.sep) || path.isAbsolute(relative)) return false;
    return relative !== '' && relative !== 'Cargo.lock';
  };
}
