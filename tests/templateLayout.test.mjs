import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { reactive, nextTick } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { parse, compileScript } from '@vue/compiler-sfc';
import { migrateTemplatePaths, templateName, BUNDLED_QUANTNIGHT_ROOT as ROOT } from '../src/utils/templatePaths.js';

const url = p => new URL(p, import.meta.url).href;
const dataModule = source => import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const vueURL = url('../node_modules/vue/dist/vue.runtime.esm-bundler.js');
const helperURL = url('../src/utils/templatePaths.js');

test('only known defaults migrate, including packaged defaults; custom and priority remain intact', () => {
  const old = { templatePath: `${ROOT}/templates/`, superTemplatePath: `${ROOT}/super_templates`, priorityTemplatePath: '/my/priority', extra: 42 };
  assert.deepEqual(migrateTemplatePaths(old, '/checkout'), {
    templatePath: '/checkout/templates/generated/regular', superTemplatePath: '/checkout/templates/generated/super', priorityTemplatePath: '/my/priority', extra: 42,
  });
  assert.equal(old.templatePath, `${ROOT}/templates/`);
  const custom = { templatePath: '/other/templates', superTemplatePath: '/other/super_templates', priorityTemplatePath: '/priority' };
  assert.deepEqual(migrateTemplatePaths(custom, '/checkout'), custom);
  const current = migrateTemplatePaths({}, '/checkout');
  assert.deepEqual(migrateTemplatePaths(current, '/checkout'), current);
  assert.equal(migrateTemplatePaths({ templatePath: './templates' }, '/checkout').templatePath, '/checkout/templates/generated/regular');
  assert.equal(migrateTemplatePaths({ superTemplatePath: './templates/super' }, '/checkout').superTemplatePath, '/checkout/templates/generated/super');
});

test('descriptor IDs strip the entire suffix and reject paths/temporary names', () => {
  for (const value of ['alpha', 'alpha.py', 'alpha.research.json']) assert.equal(templateName(value), 'alpha');
  for (const value of ['../alpha.py', '__init__.py', '.claim', 'alpha.research.json.tmp', 'helper.py~', '']) assert.throws(() => templateName(value));
});

test('both development and packaged defaults point at generated kind directories', async () => {
  for (const path of ['../src/config.js', '../src-tauri/resources/default_config.js']) {
    const source = await readFile(new URL(path, import.meta.url), 'utf8');
    assert.match(source, /templates\/generated\/regular/);
    assert.match(source, /templates\/generated\/super/);
  }
});

let serial = 0;
async function configStore(config) {
  setActivePinia(createPinia());
  const calls = [];
  const fakeInvoke = async (command, args) => {
    calls.push({ command, args });
    if (command === 'get_config_toml_content') return '[env]\nworking_dir = "/checkout"';
    if (command === 'get_config_js_content') return `export const AppConfig = ${JSON.stringify(config)};`;
    if (command === 'get_button_mappings') return '[]';
  };
  const key = `__templateConfigTest${serial++}`;
  globalThis[key] = fakeInvoke;
  const source = (await readFile(new URL('../src/stores/configStore.js', import.meta.url), 'utf8'))
    .replace("from 'pinia'", `from ${JSON.stringify(url('../node_modules/pinia/dist/pinia.mjs'))}`)
    .replace("from 'vue'", `from ${JSON.stringify(vueURL)}`)
    .replace("import { invoke } from '@tauri-apps/api/core';", `const invoke = globalThis.${key};`)
    .replace("import TOML from '@iarna/toml';", `import TOML from ${JSON.stringify(url('../node_modules/@iarna/toml/toml.js'))};`)
    .replace("from '../utils/templatePaths.js'", `from ${JSON.stringify(helperURL)}`);
  const store = (await dataModule(source)).useConfigStore();
  await store.ready;
  return { store, calls };
}

test('actual config store loads root first, persists migration with unknown keys, refresh respects custom', async () => {
  const config = { paths: { templatePath: `${ROOT}/templates`, superTemplatePath: '/custom/super' }, extension: { keep: true }, dataFilterOptions: [1] };
  const { store, calls } = await configStore(config);
  assert.equal(store.paths.templatePath, '/checkout/templates/generated/regular');
  assert.equal(store.paths.superTemplatePath, '/custom/super');
  const saved = calls.find(c => c.command === 'save_config_js_content');
  assert.ok(saved);
  assert.match(saved.args.content, /"keep": true/);
  assert.ok(calls.findIndex(c => c.command === 'get_config_toml_content') < calls.findIndex(c => c.command === 'get_config_js_content'));
  const custom = { paths: { templatePath: '/custom/regular', superTemplatePath: '/custom/super', priorityTemplatePath: '/custom/priority' } };
  const next = await configStore(custom);
  await next.store.fetchFrontendConfig();
  assert.deepEqual({ ...next.store.paths }, custom.paths);
  assert.equal(next.calls.some(c => c.command === 'save_config_js_content'), false);
});

async function modal(name, response) {
  const filename = new URL(`../src/components/modals/${name}.vue`, import.meta.url);
  const { descriptor } = parse(await readFile(filename, 'utf8'), { filename: fileURLToPath(filename) });
  const key = `__templateModalTest${serial++}`;
  const calls = [], messages = [], events = [];
  const store = { ready: Promise.resolve(), paths: { templatePath: '/custom/regular', superTemplatePath: '/custom/super' } };
  globalThis[key] = { store, invoke: async (command, args) => { calls.push({ command, args }); if (response instanceof Error) throw response; return typeof response === 'function' ? response() : response; }, message: { error: s => messages.push(s), warning: s => messages.push(s), success() {} } };
  const compiled = compileScript(descriptor, { id: name }).content
    .replace('import { useMessage } from "naive-ui";', `const useMessage = () => globalThis.${key}.message;`)
    .replace('import { invoke } from "@tauri-apps/api/core";', `const invoke = globalThis.${key}.invoke;`)
    .replace('import { useConfigStore } from "@/stores/configStore";', `const useConfigStore = () => globalThis.${key}.store;`)
    .replaceAll('"@/utils/templatePaths"', JSON.stringify(helperURL))
    .replaceAll('"vue"', JSON.stringify(vueURL));
  const component = (await dataModule(compiled)).default;
  const props = reactive({ show: false });
  const state = component.setup(props, { expose() {}, emit: (...args) => events.push(args) });
  props.show = true;
  await nextTick(); await nextTick(); await nextTick();
  return { state, calls, messages, events, props, store, template: descriptor.template.content };
}

for (const [name, kind, event] of [['AddTaskModal', 'regular', 'add-task'], ['AddSuperTaskModal', 'super', 'add-super-task']]) {
  test(`${name}: actual UI uses persisted path/kind and bare descriptor ID`, async () => {
    const { state, calls, events, template } = await modal(name, 'frozen.research.json');
    assert.deepEqual(calls[0], { command: 'read_latest_py_file', args: { folderPath: `/custom/${kind}`, taskType: kind } });
    assert.equal(state.form.value.templateFilename, 'frozen');
    state.form.value.taskName = 'test';
    state.handleSubmit();
    assert.equal(events.find(e => e[0] === event)[1].templatefile, 'frozen');
    assert.match(template, /v-model:value="form.templateFilename"/);
    assert.match(template, /accept=".py,.json"/);
    assert.doesNotMatch(template, /templateFilename }}\.py/);
  });
  test(`${name}: late discovery cannot overwrite a manual legacy name`, async () => {
    let resolve;
    const response = new Promise(r => { resolve = r; });
    const { state } = await modal(name, () => response);
    state.form.value.templateFilename = 'manual_legacy';
    resolve('latest_generated');
    await nextTick(); await nextTick();
    assert.equal(state.form.value.templateFilename, 'manual_legacy');
  });
  test(`${name}: descriptor file selection never stores JSON as Python source`, async () => {
    const { state } = await modal(name, 'initial');
    const previous = globalThis.FileReader;
    globalThis.FileReader = class {
      readAsText(file) { this.result = file.content; this.onload(); }
    };
    try {
      state.handleTemplateUpload({ file: { name: 'chosen.research.json', file: { content: '{"loader":"frozen_research"}' } } });
      assert.equal(state.form.value.templateFilename, 'chosen');
      assert.equal(state.form.value.templateCode, '');
      state.handleTemplateUpload({ file: { name: 'legacy.py', file: { content: 'async def generate_parms(): pass' } } });
      assert.equal(state.form.value.templateFilename, 'legacy');
      assert.match(state.form.value.templateCode, /generate_parms/);
      state.handleTemplateUpload({ file: { name: 'bad.tmp.json', file: { content: '' } } });
      assert.equal(state.form.value.templateFilename, 'legacy');
    } finally { globalThis.FileReader = previous; }
  });
  test(`${name}: empty discovery does not prevent manual legacy selection or overwrite custom path`, async () => {
    const { state, events, props, store, calls } = await modal(name, new Error('No templates'));
    state.form.value.templateFilename = 'legacy.py';
    state.form.value.templatePath = '/manually/changed';
    state.form.value.taskName = 'legacy-task';
    props.show = false; await nextTick();
    store.paths.templatePath = '/new/default'; store.paths.superTemplatePath = '/new/default';
    props.show = true; await nextTick(); await nextTick();
    assert.equal(state.form.value.templatePath, '/manually/changed');
    assert.equal(calls.length, 1);
    state.handleSubmit();
    assert.equal(events.find(e => e[0] === event)[1].templatefile, 'legacy');
  });
}
