import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { reactive, nextTick } from 'vue';
import { parse, compileScript } from '@vue/compiler-sfc';
import {
  PRIORITY_TEMPLATES, DEFAULT_PRIORITY_TEMPLATES, DEFAULT_PRIORITY_ROUNDS, PRIORITY_CONFIG_HINT, priorityDefaultConfig,
  resolveTaskConfig, parseTaskConfig, validatePriorityConfig, priorityTaskPayload,
} from '../src/utils/taskConfig.js';

const helperUrl = new URL('../src/utils/taskConfig.js', import.meta.url).href;

// Exercise the actual SFC setup handlers without DOM/Tauri/DB or a dev server.
async function modal(name, initialProps) {
  const url = new URL(`../src/components/modals/${name}.vue`, import.meta.url);
  const { descriptor } = parse(await readFile(url, 'utf8'), { filename: fileURLToPath(url) });
  const compiled = compileScript(descriptor, { id: name }).content
    .replace(/import \{ useMessage \} from "naive-ui";/, 'const useMessage = () => ({ error() {} });')
    .replaceAll('"@/utils/taskConfig"', JSON.stringify(helperUrl));
  // Vue imports are absolute because data: modules have no filesystem parent.
  const vueUrl = new URL('../node_modules/vue/dist/vue.runtime.esm-bundler.js', import.meta.url).href;
  const component = (await import(`data:text/javascript;base64,${Buffer.from(compiled.replaceAll('"vue"', JSON.stringify(vueUrl))).toString('base64')}`)).default;
  const props = reactive({ show: false, isPriority: true, isSuper: false, ...initialProps });
  const events = [];
  const state = component.setup(props, { expose() {}, emit: (...args) => events.push(args) });
  props.show = true;
  await nextTick();
  return { props, events, state, template: descriptor.template.content };
}

test('new priority defaults to pipeline auto with per-root stage budget 3x60 and per-seed generation cap 40', () => {
  const config = priorityDefaultConfig();
  assert.equal(config.mission_tag, 'pipeline');
  assert.equal(config.refine_templates, 'auto');
  assert.equal(config.max_variants_per_alpha, 40);
  assert.deepEqual(config.rounds, DEFAULT_PRIORITY_ROUNDS);
  assert.equal(config.rounds.enabled, true);
  assert.equal(config.rounds.max_rounds * config.rounds.max_simulations_per_round, 180);
  assert.deepEqual(DEFAULT_PRIORITY_TEMPLATES, ['ablation', 'turnover_adjust', 'operator_transfer', 'group_op_add', 'event_op_add']);
  assert.deepEqual(PRIORITY_TEMPLATES, [...DEFAULT_PRIORITY_TEMPLATES, 'datafield_replacement', 'change_settings', 'fastexpr_to_python', 'sign_flip']);
  assert.equal(PRIORITY_TEMPLATES.length, 9);
  assert.equal(DEFAULT_PRIORITY_TEMPLATES.length, 5);
  assert.match(PRIORITY_CONFIG_HINT, /9种模板中默认启用前5种 ablation、turnover_adjust、operator_transfer、group_op_add、event_op_add/);
  assert.match(PRIORITY_CONFIG_HINT, /每模板默认10条/);
  assert.match(PRIORITY_CONFIG_HINT, /每个 root 独立最多3阶段×每阶段60次实际回测，每个 root 累计最多180次/);
  assert.equal(config.source_scope, 'active');
  assert.equal(Object.hasOwn(config, 'source_task'), false);
  assert.equal(Object.hasOwn(config, 'source_tasks'), false);
  assert.match(PRIORITY_CONFIG_HINT, /每个 root 在本阶段取得实际反馈后晋级前5名/);
  assert.match(PRIORITY_CONFIG_HINT, /sign 方向规范化为免费前置/);
  assert.match(PRIORITY_CONFIG_HINT, /独立 ablation 只消融结构并保留 settings/);
  assert.match(PRIORITY_CONFIG_HINT, /turnover_adjust 仅产生0\/1条 decay-only 变体/);
  assert.match(PRIORITY_CONFIG_HINT, /sign_flip 登记在最后，适用于legacy纯翻转/);
  assert.match(PRIORITY_CONFIG_HINT, /max_variants_per_alpha 默认40/);
  assert.match(PRIORITY_CONFIG_HINT, /num_variants 默认6、max_attempts 默认60/);
  assert.deepEqual(validatePriorityConfig(config), config);
});

test('unconfigured legacy sign_flip is never expanded; named pipeline opts in', () => {
  for (const task of [{ name: 'sign_flip__priority' }, { name: 'custom__priority', templatefile: 'sign_flip' }, {}]) {
    const config = priorityDefaultConfig(task);
    assert.equal(Object.hasOwn(config, 'refine_templates'), false);
    assert.equal(Object.hasOwn(config, 'mission_tag'), false);
    assert.equal(Object.hasOwn(config, 'rounds'), false);
  }
  assert.equal(priorityDefaultConfig({ name: 'custom__priority', templatefile: 'pipeline' }).refine_templates, 'auto');
});

test('saved config priority is mission_config > startConfig > config, including JSON strings', () => {
  const defaults = priorityDefaultConfig();
  assert.deepEqual(resolveTaskConfig({ mission_config: {}, startConfig: defaults }, defaults), {});
  assert.deepEqual(resolveTaskConfig({ mission_config: null, startConfig: '{"refine_templates":[]}', config: defaults }, defaults), { refine_templates: [] });
  assert.deepEqual(resolveTaskConfig({ config: '{"interval":33}' }, defaults), { interval: 33 });
  assert.deepEqual(resolveTaskConfig({}, defaults), defaults);
});

test('saved configs are cloned without injecting pipeline defaults or losing extension params', () => {
  const saved = { refine_templates: [], extension: { arbitrary: ['preserved'] } };
  const output = resolveTaskConfig({ mission_config: saved }, priorityDefaultConfig());
  output.extension.arbitrary.push('local edit');
  assert.deepEqual(saved.extension.arbitrary, ['preserved']);
  assert.deepEqual(output.refine_templates, []);
  assert.equal(Object.hasOwn(output, 'mission_tag'), false);
  assert.equal(Object.hasOwn(output, 'rounds'), false);
});

test('invalid saved config is surfaced instead of silently falling back', () => {
  for (const value of ['broken', '[]', 'null', [], false, 3]) {
    assert.throws(() => resolveTaskConfig({ mission_config: value }, priorityDefaultConfig()));
  }
});

test('advanced step schema preserves switches, params, disabled expensive templates and caps', () => {
  const config = {
    refine_templates: ['sign_flip', { name: 'change_settings', enabled: false, params: { regions: ['USA'] }, max_variants: 3 }],
    max_variants_per_alpha: 40, unknownExtension: { keep: true },
  };
  assert.deepEqual(validatePriorityConfig(JSON.stringify(config)), config);
  assert.deepEqual(validatePriorityConfig({ refine_templates: [] }), { refine_templates: [] });
  assert.deepEqual(validatePriorityConfig({ refine_templates: 'sign_flip, turnover_adjust' }), { refine_templates: 'sign_flip, turnover_adjust' });
});

test('event steps accept names, disabled steps and advanced params without injecting defaults', () => {
  for (const refine_templates of ['event_op_add', ['event_op_add'], [{ name: 'event_op_add' }], [{ name: 'event_op_add', enabled: false, params: {} }]]) {
    const config = { refine_templates };
    assert.deepEqual(validatePriorityConfig(config), config);
  }
  for (const include_regional of [false, true]) {
    const config = {
      refine_templates: [{ name: 'event_op_add', enabled: true, params: {
        open_events: ['volume > adv20', 'ts_rank(returns, 20) > 0.5'],
        exit_events: ['-1', 'abs(returns) > 0.1'],
        num_variants: 6, max_attempts: 60, include_regional,
      }, max_variants: 10 }],
      max_variants_per_alpha: 40,
    };
    const output = validatePriorityConfig(JSON.stringify(config));
    assert.deepEqual(output, config);
    output.refine_templates[0].params.open_events.push('local edit');
    assert.equal(config.refine_templates[0].params.open_events.length, 2);
  }
  assert.deepEqual(validatePriorityConfig({ refine_templates: ['sign_flip'] }), { refine_templates: ['sign_flip'] });
});

for (const key of ['num_variants', 'max_attempts']) {
  for (const value of [null, 0, -1, 1.5, true, '6', Number.MAX_SAFE_INTEGER + 1]) {
    test(`event validation rejects ${key}=${JSON.stringify(value)}`, () => {
      assert.throws(() => validatePriorityConfig({ refine_templates: [{ name: 'event_op_add', params: { [key]: value } }] }), /必须为正整数/);
    });
  }
}

for (const key of ['open_events', 'exit_events']) {
  for (const value of [null, [], 'returns > 0', [null], [1], [''], ['  '], ['returns > 0', false]]) {
    test(`event validation rejects ${key}=${JSON.stringify(value)}`, () => {
      assert.throws(() => validatePriorityConfig({ refine_templates: [{ name: 'event_op_add', params: { [key]: value } }] }), /必须为非空字符串列表/);
    });
  }
}

for (const value of [null, 0, 1, 'false', [], {}]) {
  test(`event validation rejects include_regional=${JSON.stringify(value)}`, () => {
    assert.throws(() => validatePriorityConfig({ refine_templates: [{ name: 'event_op_add', params: { include_regional: value } }] }), /必须为布尔值/);
  });
}

for (const [label, value] of [
  ['top-level array', []], ['null config', null], ['null steps', { refine_templates: null }],
  ['invalid generation cap', { refine_templates: [{ name: 'change_settings', params: { max_attempts: null } }] }],
  ['invalid group count', { refine_templates: [{ name: 'group_op_add', params: { num_variants: 0 } }] }],
  ['non-list steps', { refine_templates: 1 }],
  ['unknown template', { refine_templates: ['unknown'] }], ['empty string', { refine_templates: '' }],
  ['duplicate steps', { refine_templates: ['sign_flip', 'sign_flip'] }],
  ['invalid step', { refine_templates: [null] }], ['unknown key', { refine_templates: [{ name: 'sign_flip', typo: 1 }] }],
  ['nonboolean enabled', { refine_templates: [{ name: 'sign_flip', enabled: 0 }] }],
  ['array params', { refine_templates: [{ name: 'sign_flip', params: [] }] }],
  ['reserved params', { refine_templates: [{ name: 'sign_flip', params: { auth_profile: 'user2' } }] }],
  ['zero step cap', { refine_templates: [{ name: 'sign_flip', max_variants: 0 }] }],
  ['negative cap', { max_variants_per_alpha: -1 }], ['fractional cap', { max_variants_per_alpha: 1.5 }],
  ['boolean cap', { max_variants_per_alpha: true }],
]) {
  test(`priority validation rejects ${label}`, () => assert.throws(() => validatePriorityConfig(value)));
}

test('new task uses pipeline template independent of task name and persists only mission_config', () => {
  const task = priorityTaskPayload(' custom-research__priority ', ' user2 ', '{"refine_templates":[]}');
  assert.equal(task.name, 'custom-research__priority');
  assert.equal(task.templatefile, 'pipeline');
  assert.equal(task.auth_profile, 'user2');
  assert.deepEqual(task.mission_config, { refine_templates: [], mission_tag: 'pipeline' });
  assert.equal(Object.hasOwn(task, 'config'), false);
  assert.throws(() => priorityTaskPayload('../bad', 'user1', '{}'));
  assert.throws(() => priorityTaskPayload('_bad', 'user1', '{}'));
  assert.throws(() => priorityTaskPayload('a'.repeat(101), 'user1', '{}'));
  assert.deepEqual(priorityTaskPayload('custom', '', '{}').mission_config, { mission_tag: 'pipeline' });
  assert.equal(priorityTaskPayload('legacy', '', '{"mission_tag":"sign_flip"}').mission_config.mission_tag, 'sign_flip');
});

test('create modal has editable JSON and emits the event consumed by TaskManager', async () => {
  const { state, events, template } = await modal('AddPriorityTaskModal');
  assert.match(template, /v-model:value="form.configText"/);
  assert.equal(JSON.parse(state.form.value.configText).refine_templates, 'auto');
  assert.deepEqual(JSON.parse(state.form.value.configText).rounds, DEFAULT_PRIORITY_ROUNDS);
  state.form.value.taskName = 'my_research__priority';
  state.form.value.configText = '{"refine_templates":[]}';
  state.handleSubmit();
  const created = events.find(([name]) => name === 'add-priority-task');
  assert.equal(created[1].templatefile, 'pipeline');
  assert.deepEqual(created[1].mission_config.refine_templates, []);
});

test('create modal accepts event JSON and rejects illegal event budget without emitting', async () => {
  const config = { refine_templates: [{ name: 'event_op_add', params: { num_variants: 6, max_attempts: 60, include_regional: false } }] };
  const { state, events } = await modal('AddPriorityTaskModal');
  state.form.value.taskName = 'event_research';
  state.form.value.configText = JSON.stringify(config);
  state.handleSubmit();
  assert.deepEqual(events.find(([name]) => name === 'add-priority-task')[1].mission_config, { ...config, mission_tag: 'pipeline' });
  const count = events.length;
  config.refine_templates[0].params.max_attempts = 0;
  state.form.value.configText = JSON.stringify(config);
  state.handleSubmit();
  assert.equal(events.length, count);
});

for (const [name, handler, event] of [
  ['StartTaskModal', 'onConfirm', 'start-priority-task'],
  ['UpdateTaskModal', 'onConfirm', 'update-priority-task'],
]) {
  test(`${name} round-trips explicit advanced JSON and [] without defaults`, async () => {
    const saved = { refine_templates: [{ name: 'group_op_add', enabled: true, params: { num_variants: 2 }, max_variants: 4 }, { name: 'event_op_add', params: { open_events: ['volume > adv20'], exit_events: ['-1'], num_variants: 6, max_attempts: 60, include_regional: false }, max_variants: 3 }], max_variants_per_alpha: 9, extra: { keep: true } };
    const { state, events, props } = await modal(name, { task: { name: 'custom__priority', isRemote: false, mission_config: saved } });
    if (name === 'StartTaskModal') { assert.equal(state.form.value.config, ''); state.setConfigMode(true); state.form.value.config = JSON.stringify(saved); }
    assert.deepEqual(JSON.parse(state.form.value.config), saved);
    state[handler]();
    assert.deepEqual(events.find(([key]) => key === event)[1].config, saved);
    props.show = false;
    await nextTick();
    props.task = { name: 'custom__priority', startConfig: { refine_templates: [] } };
    props.show = true;
    await nextTick();
    if (name === 'StartTaskModal') { assert.equal(state.editConfig.value, false); assert.equal(state.form.value.config, ''); state.setConfigMode(true); state.form.value.config = '{"refine_templates":[]}'; }
    assert.deepEqual(JSON.parse(state.form.value.config), { refine_templates: [] });
  });

  test(`${name} preserves legacy unconfigured single-template task`, async () => {
    const { state } = await modal(name, { task: { name: 'sign_flip__priority', templatefile: 'sign_flip' } });
    if (name === 'StartTaskModal') { assert.equal(state.editConfig.value, false); assert.equal(state.form.value.config, ''); }
    else assert.equal(Object.hasOwn(JSON.parse(state.form.value.config), 'refine_templates'), false);
  });

  test(`${name} retains saved legacy filters without injecting auto templates`, async () => {
    const saved = { interval: 30, filter_params: { sharpe_th: 1.8 } };
    const { state } = await modal(name, { task: { name: 'sign_flip__priority', mission_config: saved } });
    if (name === 'StartTaskModal') { assert.equal(state.form.value.config, ''); state.setConfigMode(true); state.form.value.config = JSON.stringify(saved); }
    assert.deepEqual(JSON.parse(state.form.value.config), saved);
  });

  test(`${name} rejects invalid config without emitting a start/update command`, async () => {
    const { state, events } = await modal(name, { task: { name: 'pipeline__priority', templatefile: 'pipeline' } });
    for (const config of ['[]', '{"refine_templates":[{"name":"event_op_add","params":{"num_variants":0}}]}', '{"refine_templates":[{"name":"event_op_add","params":{"max_attempts":-1}}]}']) {
      if (name === 'StartTaskModal') state.setConfigMode(true);
      state.form.value.config = config;
      state[handler]();
      assert.equal(events.some(([key]) => key === event), false);
    }
  });
}

const roundDefaults = {
  enabled: true, max_rounds: 3, max_simulations_per_round: 60, promote_top_k: 5,
  exploration_fraction: 0.2, wait_timeout: 7200, poll_interval: 30,
};

test('round schema defaults are exact and independently cloned for new pipeline/auto tasks', () => {
  assert.deepEqual(DEFAULT_PRIORITY_ROUNDS, roundDefaults);
  for (const templatefile of ['pipeline', 'auto']) {
    const config = priorityDefaultConfig({ templatefile });
    assert.deepEqual(config.rounds, roundDefaults);
    config.rounds.max_rounds = 1;
    assert.deepEqual(priorityDefaultConfig({ templatefile }).rounds, roundDefaults);
  }
});

test('round validation preserves partial, disabled and omitted configs without defaults injection', () => {
  for (const rounds of [null, {}, { enabled: false }, { enabled: true }, roundDefaults,
    { ...roundDefaults, enabled: false },
    { max_rounds: 1, max_simulations_per_round: 1, promote_top_k: 1, exploration_fraction: 0, wait_timeout: 0.5, poll_interval: 0.25 },
    { exploration_fraction: 1 }]) {
    const config = { rounds, extension: { keep: ['exact'] } };
    const output = validatePriorityConfig(config);
    assert.deepEqual(output, config); // Also validates before missing-steps early return.
    output.extension.keep.push('local');
    assert.deepEqual(config.extension.keep, ['exact']);
  }
  assert.deepEqual(validatePriorityConfig({}), {});
  assert.deepEqual(priorityTaskPayload('explicit-legacy', '', '{}').mission_config, { mission_tag: 'pipeline' });
});

const badRounds = [[], 'auto', true, 1, { unknown: 1 }, { order: ['sign_flip'] }];
for (const enabled of [null, 0, 1, 'true', [], {}]) badRounds.push({ enabled });
for (const key of ['max_rounds', 'max_simulations_per_round', 'promote_top_k']) {
  for (const value of [null, true, false, 0, -1, 1.5, '3', NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) badRounds.push({ [key]: value });
}
badRounds.push({ max_rounds: 4 }, { max_simulations_per_round: 61 });
for (const key of ['exploration_fraction', 'wait_timeout', 'poll_interval']) {
  for (const value of [null, true, false, '0.2', NaN, Infinity, -Infinity, -1]) badRounds.push({ [key]: value });
}
badRounds.push({ exploration_fraction: 1.01 }, { wait_timeout: 0 }, { poll_interval: 0 });
for (const [index, rounds] of badRounds.entries()) {
  for (const disabled of [false, true]) {
    test(`round validation rejects invalid case ${index}, disabled=${disabled}`, () => {
      const raw = rounds && !Array.isArray(rounds) && typeof rounds === 'object'
        ? { enabled: !disabled, ...rounds } : rounds;
      assert.throws(() => validatePriorityConfig({ rounds: raw }));
      assert.throws(() => validatePriorityConfig({ rounds: raw, refine_templates: [] }));
    });
  }
}

for (const [name, handler, event] of [
  ['StartTaskModal', 'onConfirm', 'start-priority-task'],
  ['UpdateTaskModal', 'onConfirm', 'update-priority-task'],
]) {
  test(`${name} round-trips explicit round configs; Start default defers, Update keeps original default`, async () => {
    for (const saved of [{ rounds: { ...roundDefaults, enabled: false }, refine_templates: [] },
      { rounds: { enabled: true, max_rounds: 2 }, refine_templates: ['ablation'] }]) {
      const { state, events } = await modal(name, { task: { name: 'wholepriority', templatefile: 'pipeline', mission_config: saved } });
      if (name === 'StartTaskModal') { assert.equal(state.form.value.config, ''); state.setConfigMode(true); state.form.value.config = JSON.stringify(saved); }
    assert.deepEqual(JSON.parse(state.form.value.config), saved);
      state[handler]();
      assert.deepEqual(events.find(([key]) => key === event)[1].config, saved);
    }
    const { state } = await modal(name, { task: { name: 'wholepriority', templatefile: 'pipeline' } });
    if (name === 'StartTaskModal') { assert.equal(state.editConfig.value, false); assert.equal(state.form.value.config, ''); }
    else assert.deepEqual(JSON.parse(state.form.value.config).rounds, roundDefaults);
  });

  test(`${name} rejects rounds before emitting, including disabled schema and early-return legacy config`, async () => {
    const { state, events } = await modal(name, { task: { name: 'wholepriority', templatefile: 'pipeline' } });
    for (const rounds of [{ max_rounds: 4 }, { max_simulations_per_round: 61 }, { enabled: false, exploration_fraction: 2 }, { order: [] }]) {
      if (name === 'StartTaskModal') state.setConfigMode(true);
      state.form.value.config = JSON.stringify({ rounds });
      state[handler]();
      assert.equal(events.some(([key]) => key === event), false);
    }
  });
}


test('direction mode reserves implementation flags and ablation_only is removed in every mode', () => {
  for (const mode of [{ rounds: { enabled: true } }, { direction_normalization: true }]) {
    assert.throws(() => validatePriorityConfig({ ...mode,
      refine_templates: [{ name: 'ablation', params: { direction_normalization: false } }] }), /内部参数/);
  }
  for (const mode of [{}, { rounds: { enabled: false } }, { rounds: { enabled: true } }, { direction_normalization: true }]) {
    for (const name of ['sign_flip', 'turnover_adjust', 'ablation']) {
      assert.throws(() => validatePriorityConfig({ ...mode,
        refine_templates: [{ name, params: { ablation_only: false } }] }));
    }
  }
});

for (const mode of [{ rounds: { enabled: true } }, { direction_normalization: true },
  { rounds: { enabled: false }, direction_normalization: true }]) {
  test(`enabled sign_flip fails early without silent migration: ${JSON.stringify(mode)}`, () => {
    for (const refine_templates of ['sign_flip', 'ablation, sign_flip', ['sign_flip'],
      [{ name: 'sign_flip' }], [{ name: 'sign_flip', enabled: true }]]) {
      const config = { ...mode, refine_templates };
      const before = JSON.stringify(config);
      assert.throws(() => validatePriorityConfig(config), /sign flip是免费前置，请用ablation\/auto/);
      assert.throws(() => priorityTaskPayload('direction-test', '', JSON.stringify(config)), /sign flip是免费前置/);
      assert.equal(JSON.stringify(config), before);
    }
    for (const refine_templates of ['auto', ['ablation'], [], [{ name: 'sign_flip', enabled: false }]]) {
      const config = { ...mode, refine_templates };
      assert.deepEqual(validatePriorityConfig(config), config);
    }
  });
}

test('legacy pure sign_flip and independent ablation keep explicit configuration, no compatibility mode', () => {
  for (const mode of [{}, { rounds: { enabled: false } }, { direction_normalization: false }]) {
    for (const refine_templates of ['sign_flip', ['sign_flip'], [{ name: 'sign_flip', enabled: true }],
      'ablation,turnover_adjust', [{ name: 'ablation', max_variants: 4, params: {} }]]) {
      const config = { ...mode, refine_templates };
      assert.deepEqual(validatePriorityConfig(config), config);
      assert.equal(Object.hasOwn(validatePriorityConfig(config), 'compatMode'), false);
    }
  }
  for (const direction_normalization of [null, 0, 1, 'true', [], {}]) {
    assert.throws(() => validatePriorityConfig({ direction_normalization }), /必须为布尔值/);
  }
});

for (const [name, handler, event] of [
  ['AddPriorityTaskModal', 'handleSubmit', 'add-priority-task'],
  ['StartTaskModal', 'onConfirm', 'start-priority-task'],
  ['UpdateTaskModal', 'onConfirm', 'update-priority-task'],
]) {
  test(`${name} rejects saved round sign_flip; explicit ablation replacement emits without resetting budget`, async () => {
    const old = { rounds: { enabled: true, max_rounds: 2, max_simulations_per_round: 17 }, refine_templates: ['sign_flip'] };
    const { state, events } = await modal(name, { task: { name: 'split-test', templatefile: 'pipeline', mission_config: old } });
    const key = name === 'AddPriorityTaskModal' ? 'configText' : 'config';
    if (name === 'AddPriorityTaskModal') state.form.value.taskName = 'split-test';
    if (name === 'StartTaskModal') state.setConfigMode(true);
    state.form.value[key] = JSON.stringify(old);
    state[handler]();
    assert.equal(events.some(([key]) => key === event), false);
    assert.deepEqual(JSON.parse(state.form.value[key]), old);
    const replacement = { ...old, refine_templates: ['ablation'] };
    state.form.value[key] = JSON.stringify(replacement);
    state[handler]();
    const payload = events.find(([key]) => key === event)[1];
    const saved = name === 'AddPriorityTaskModal' ? payload.mission_config : payload.config;
    assert.deepEqual(saved.refine_templates, ['ablation']);
    assert.deepEqual(saved.rounds, old.rounds);
  });
}


test('dynamic sources remain omitted; optional list validates even before template fallback', () => {
  for (const config of [{}, { source_scope: 'active' }, { source_tasks: ['task-a', 'task-b'] }]) {
    assert.deepEqual(validatePriorityConfig(config), config);
  }
  for (const source_tasks of [null, [], '', [''], ['  '], [1], ['a', null], {}]) {
    for (const refine_templates of [undefined, []]) {
      assert.throws(() => validatePriorityConfig({ source_tasks, refine_templates }), /source_tasks/);
    }
  }
  for (const source_scope of [null, false, 'all', 'fixed']) {
    assert.throws(() => validatePriorityConfig({ source_scope }), /source_scope/);
  }
  const saved = { source_task: 'legacy-origin', source_remote: true, source_tasks: ['a'],
    extension: { scope: { dataset: 'keep' } }, rounds: { enabled: true } };
  assert.deepEqual(resolveTaskConfig({ mission_config: saved }, priorityDefaultConfig()), saved);
  assert.deepEqual(validatePriorityConfig(saved), saved); // no silent source migration
  assert.match(PRIORITY_CONFIG_HINT, /protocol3/);
  assert.match(PRIORITY_CONFIG_HINT, /来源任务 normal consumer 需支持 protocol3，缺少该支持时处于等待阶段/);
});


test('selected Priority CP host is independent from immutable source execution', () => {
  assert.match(PRIORITY_CONFIG_HINT, /所选 Priority 主机 control 注册表中同 auth/);
  assert.match(PRIORITY_CONFIG_HINT, /前端负责启动和监控/);
  assert.match(PRIORITY_CONFIG_HINT, /CP目标独立于来源执行/);
  assert.match(PRIORITY_CONFIG_HINT, /来源按root.source_remote路由/);
  assert.doesNotMatch(PRIORITY_CONFIG_HINT, /始终本地|固定本地/);
});

test('create host defaults remote and round-trips both hosts without rewriting root origin', async () => {
  assert.equal(priorityTaskPayload('remote-default', '', '{}').is_remote, true);
  for (const isRemote of [true, false]) {
    const { state, events, template } = await modal('AddPriorityTaskModal');
    assert.equal(state.form.value.isRemote, true);
    assert.match(template, /v-model:value="form.isRemote"/);
    assert.doesNotMatch(template, /固定本地/);
    state.form.value.isRemote = isRemote;
    state.form.value.configText = JSON.stringify({ rounds: { enabled: true }, source_remote: !isRemote });
    state.handleSubmit();
    const created = events.find(([name]) => name === 'add-priority-task')[1];
    assert.equal(created.is_remote, isRemote);
    assert.equal(created.mission_config.source_remote, !isRemote);
    assert.equal(created.mission_config.rounds.enabled, true);
  }
  assert.throws(() => priorityTaskPayload('bad-host', '', '{}', 'true'), /运行主机/);
});

for (const [name, handler, eventName] of [
  ['StartTaskModal', 'onConfirm', 'start-priority-task'],
  ['UpdateTaskModal', 'onConfirm', 'update-priority-task'],
]) {
  test(`${name} forwards the requested per-root host independently of source_remote`, async () => {
    for (const isRemote of [true, false]) {
      const saved = { rounds: { enabled: true }, source_remote: !isRemote };
      const { state, events } = await modal(name, { task: { name: 'remote__priority', isRemote, mission_config: saved } });
      state[handler]();
      const sent = events.find(([key]) => key === eventName)[1];
      assert.equal(sent.isRemote, isRemote);
      if (name === 'StartTaskModal') assert.equal(Object.hasOwn(sent, 'config'), false);
      else assert.deepEqual(sent.config, saved);
    }
  });
}

// Scheduling weight is the existing priority field, not a stage/generation cap.
test('priority is a finite positive scheduling weight, default 1, independent of budgets', () => {
  assert.equal(priorityDefaultConfig().priority, 1);
  assert.match(PRIORITY_CONFIG_HINT, /priority（默认1）为有限正数调度权重/);
  assert.match(PRIORITY_CONFIG_HINT, /数值越大越优先；先任务优先级，再R3→R2→R1，同阶段已开始root优先；正在运行的模拟继续执行/);
  for (const priority of [0.5, 1, 2, 200, 1e100]) {
    const config = { priority, rounds: { enabled: true, max_rounds: 2, max_simulations_per_round: 17 },
      max_variants_per_alpha: 9, refine_templates: [] };
    assert.deepEqual(validatePriorityConfig(config), config);
    assert.deepEqual(validatePriorityConfig(JSON.stringify(config)), config);
    assert.deepEqual(priorityTaskPayload('weight-test', '', JSON.stringify(config)).mission_config,
      { ...config, mission_tag: 'pipeline' });
  }
  assert.deepEqual(validatePriorityConfig({}), {}); // legacy omission is not expanded
  for (const priority of [null, false, true, 0, -1, '2', {}, [], NaN, Infinity, -Infinity]) {
    for (const extra of [{}, { refine_templates: [] }, { rounds: { enabled: false } }]) {
      assert.throws(() => validatePriorityConfig({ priority, ...extra }), /priority（调度优先级）必须为有限正数/);
    }
  }
  assert.throws(() => validatePriorityConfig('{"priority":1e999}'), /必须为有限正数/);
});

for (const [name, handler, event] of [
  ['StartTaskModal', 'onConfirm', 'start-priority-task'],
  ['UpdateTaskModal', 'onConfirm', 'update-priority-task'],
]) {
  test(`${name} preserves saved priority and edits only weight without changing business budgets`, async () => {
    const saved = { priority: 7.5, rounds: { enabled: true, max_rounds: 2, max_simulations_per_round: 17 },
      max_variants_per_alpha: 9, refine_templates: [], filter_params: { sharpe_th: 1.8 } };
    for (const key of ['mission_config', 'startConfig', 'config']) {
      const { state, events } = await modal(name, { task: { name: 'weight-test', isRemote: false,
        templatefile: 'pipeline', [key]: JSON.stringify(saved) } });
      if (name === 'StartTaskModal') { assert.equal(state.form.value.config, ''); state.setConfigMode(true); state.form.value.config = JSON.stringify(saved); }
    assert.deepEqual(JSON.parse(state.form.value.config), saved);
      const edited = { ...JSON.parse(state.form.value.config), priority: 250 };
      state.form.value.config = JSON.stringify(edited);
      state[handler]();
      const payload = events.find(([key]) => key === event)[1];
      assert.deepEqual(payload.config, { ...saved, priority: 250 });
      assert.equal(payload.isRemote, false);
      assert.equal(saved.priority, 7.5);
    }
  });
}

for (const [name, handler, event] of [
  ['AddPriorityTaskModal', 'handleSubmit', 'add-priority-task'],
  ['StartTaskModal', 'onConfirm', 'start-priority-task'],
  ['UpdateTaskModal', 'onConfirm', 'update-priority-task'],
]) {
  test(`${name} rejects invalid priority without emitting a command`, async () => {
    const { state, events } = await modal(name, { task: { name: 'weight-test', templatefile: 'pipeline' } });
    const field = name === 'AddPriorityTaskModal' ? 'configText' : 'config';
    for (const priority of ['0', '-1', 'null', 'true', '"2"', '1e999', '[]', '{}']) {
      if (name === 'StartTaskModal') state.setConfigMode(true);
      state.form.value[field] = `{"priority":${priority}}`;
      state[handler]();
      assert.equal(events.some(([key]) => key === event), false);
    }
  });
}

test('configuration copy retains actionable formats and bounds without defensive prohibitions', () => {
  assert.doesNotMatch(PRIORITY_CONFIG_HINT, /不|禁止|standalone|wrong-route|UNKNOWN|全局/);
  for (const detail of ['rounds.enabled:true', 'rounds.max_rounds', 'max_simulations_per_round', '20%探索',
    'max_variants_per_alpha 默认40', 'source_scope 默认 active', 'source_tasks', 'source_task/source_remote',
    'enabled:false', '[] 全部关闭', 'open_events / exit_events', 'num_variants 默认6', 'max_attempts 默认60', 'include_regional']) {
    assert.ok(PRIORITY_CONFIG_HINT.includes(detail));
  }
  assert.throws(() => parseTaskConfig([]), /配置必须是 JSON 对象（例如 {}）/);
  assert.throws(() => validatePriorityConfig({ refine_templates: null }), /refine_templates 必须为 auto、逗号分隔字符串或数组/);
  assert.throws(() => validatePriorityConfig({ refine_templates: [{ name: 'ablation', params: { auth_profile: 'other' } }] }),
    /ablation.params 必须为模板参数对象；runner 的身份\/内部参数由运行器管理/);
});

test('v4 Start explicit mode resets on task/reopen and withdrawal, never inferred from JSON equality', async () => {
  const { state, events, props, template } = await modal('StartTaskModal', { task: { _id: 'a', name: 'first', mission_config: { priority: 3 } } });
  assert.equal(state.editConfig.value, false); assert.equal(state.form.value.config, '');
  assert.match(template, /不展示推断的有效 JSON/);
  state.setConfigMode(true); state.form.value.config = '{}'; state.onConfirm();
  assert.deepEqual(events.find(([key]) => key === 'start-priority-task')[1].config, {}, 'explicit empty object stays explicit');
  props.show = false; await nextTick(); props.show = true; await nextTick();
  assert.equal(state.editConfig.value, false); assert.equal(state.form.value.config, '');
  state.setConfigMode(true); state.form.value.config = '{"priority":2}';
  props.task = { _id: 'b', name: 'replacement', mission_config: { priority: 8 } }; await nextTick();
  assert.equal(state.editConfig.value, false); assert.equal(state.form.value.config, '');
  state.setConfigMode(true); state.form.value.config = '{"priority":2}'; state.setConfigMode(false);
  assert.equal(state.form.value.config, ''); state.onConfirm();
  const last = events.filter(([key]) => key === 'start-priority-task').at(-1)[1];
  assert.equal(last.taskName, 'replacement'); assert.equal(Object.hasOwn(last, 'config'), false);
  state.setConfigMode(true); state.form.value.config = '{"priority":2}';
  props.task.name = 'changed-in-place'; await nextTick();
  assert.equal(state.editConfig.value, false); assert.equal(state.form.value.config, '');
});

for (const type of ['regular', 'super', 'priority']) test(`v4 Start ${type}: explicit invalid JSON never silently becomes None`, async () => {
  const { state, events } = await modal('StartTaskModal', { isPriority: type === 'priority', isSuper: type === 'super', task: { name: 'fixture', taskType: type } });
  state.setConfigMode(true);
  for (const input of ['null', '[]', 'false', '"null"', 'broken', '']) {
    state.form.value.config = input; state.onConfirm();
    assert.equal(events.some(([key]) => key.startsWith('start-')), false);
    assert.equal(state.editConfig.value, true); assert.equal(state.form.value.config, input);
  }
  const explicit = { opaque: { $oid: 'business', $numberInt: '01', values: [null, true] } };
  state.form.value.config = JSON.stringify(explicit); state.onConfirm();
  assert.deepEqual(events.find(([key]) => key.startsWith('start-'))[1].config, explicit);
});
