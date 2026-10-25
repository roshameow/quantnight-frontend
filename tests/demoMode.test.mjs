import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, realpathSync, readFileSync, writeFileSync, symlinkSync, rmSync, lstatSync } from 'node:fs';
import { Server } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer, loadConfigFromFile } from 'vite';

const source = fileURLToPath(new URL('../', import.meta.url));
const taskPlugin = 'quantnight-dev-rust-inputs';
function fixture(t) {
  const root = realpathSync(mkdtempSync(path.join(os.tmpdir(), 'qn-demo-config-')));
  t.after(() => rmSync(root, { recursive:true, force:true }));
  mkdirSync(path.join(root, 'scripts'));
  for (const file of ['vite.config.js', 'package.json', 'demo.html', 'scripts/dev-inputs.mjs', 'scripts/dev-registration.mjs']) {
    copyFileSync(path.join(source, file), path.join(root, file));
  }
  // Reuse installed dependencies, never install or write into their cache.
  // Keep node_modules itself local so Vite's .vite-temp stays disposable too.
  mkdirSync(path.join(root, 'node_modules/@vitejs'), {recursive:true});
  for (const name of ['vite', '@vitejs/plugin-vue']) {
    symlinkSync(realpathSync(path.join(source, 'node_modules', name)), path.join(root, 'node_modules', name));
  }
  let listens = 0;
  t.mock.method(Server.prototype, 'listen', () => { listens++; throw new Error('fixture must not open a socket'); });
  t.after(() => assert.equal(listens, 0));
  return root;
}
const load = (root, mode) => loadConfigFromFile({ command:'serve', mode }, path.join(root,'vite.config.js'), root, 'silent');
const plugins = config => config.plugins.flat(Infinity).filter(Boolean).map(p => p.name);
function start(root, config, mode) {
  return createServer({ ...config, mode, root, configFile:false, envFile:false, publicDir:false,
    cacheDir:path.join(root,'.vite-cache'), logLevel:'silent',
    optimizeDeps:{ noDiscovery:true, include:[] },
    server:{ ...config.server, middlewareMode:true, hmr:false, ws:false, watch:null, preTransformRequests:false },
  });
}
function noEnrollmentWrites(root) {
  for (const name of ['.quantnight', 'src/config.js', 'src-tauri/src/dev-input.stamp']) {
    assert.equal(lstatSync(path.join(root,name), {throwIfNoEntry:false}), undefined, name);
  }
}

test('original demo script selects explicit demo mode; real config omits only taskctl plugin and serves without backend/enrollment', {timeout:15000}, async t => {
  const root = fixture(t);
  const pkg = JSON.parse(readFileSync(path.join(root,'package.json')));
  assert.equal(pkg.scripts.demo, 'vite --mode demo --open /demo.html');
  assert.equal(pkg.scripts.dev, 'vite');
  assert.equal(pkg.scripts['tauri:dev'], 'tauri dev');
  const demo = (await load(root, 'demo')).config;
  const desktop = (await load(root, 'development')).config;
  assert.ok(plugins(desktop).includes(taskPlugin));
  assert.deepEqual(plugins(demo), plugins(desktop).filter(name => name !== taskPlugin));
  assert.ok(plugins(demo).includes('vite:vue'));
  assert.ok(plugins(demo).includes('ensure-config-js'));
  const server = await start(root, demo, 'demo');
  try {
    assert.equal(server.httpServer, null);
    assert.deepEqual(server.watcher.getWatched(), {});
    const html = await server.transformIndexHtml('/demo.html', readFileSync(path.join(root,'demo.html'),'utf8'));
    assert.match(html, /Synthetic Data Demo/);
    assert.match(html, /src\/demo\/main.js/);
    noEnrollmentWrites(root);
  } finally { await server.close(); }
});

for (const mode of ['development', 'custom-desktop']) test(`real ${mode} server still refuses missing enrollment`, {timeout:15000}, async t => {
  const root = fixture(t);
  const { config } = await load(root, mode);
  assert.ok(plugins(config).includes(taskPlugin));
  // Close any unexpectedly successful server before failing the oracle.
  await assert.rejects(async () => {
    const server = await start(root, config, mode);
    await server.close();
  }, /taskctl enrollment unavailable/);
  noEnrollmentWrites(root);
});

test('counterexample: restoring unconditional taskctl plugin breaks backend-free demo through the real config/server', {timeout:15000}, async t => {
  const root = fixture(t);
  const file = path.join(root,'vite.config.js');
  const fixed = readFileSync(file,'utf8');
  const broken = fixed.replace("...(mode === 'demo' ? [] : [devInputs(__dirname)])", 'devInputs(__dirname)');
  assert.notEqual(broken, fixed, 'counterexample must actually change the disposable config');
  writeFileSync(file, broken);
  const { config } = await load(root,'demo');
  assert.ok(plugins(config).includes(taskPlugin));
  await assert.rejects(async () => {
    const server = await start(root,config,'demo');
    await server.close();
  }, /taskctl enrollment unavailable/);
  noEnrollmentWrites(root);
});
