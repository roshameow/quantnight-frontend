import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createSSRApp, h } from 'vue';
import { renderToString } from '@vue/server-renderer';
import { createPinia, setActivePinia } from 'pinia';
import { useTaskStore } from '../src/stores/taskStore.js';
import { businessDiagnostic, taskDiagnosticSummary } from '../src/utils/taskDiagnostics.js';

const require = createRequire(import.meta.url);
const vue = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const source = await readFile(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
const manager = await import(`data:text/javascript;base64,${Buffer.from(parse(source).descriptor.script.content
  .replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))).toString('base64')}`);
const compiled = compileScript(parse(await readFile(new URL('../src/components/TaskCards/RegularTaskCard.vue', import.meta.url), 'utf8')).descriptor,
  { id: 'business-diagnostics', inlineTemplate: true }).content
  .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
  .replaceAll("'../../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
  .replaceAll('from "vue"', `from ${JSON.stringify(vue)}`).replaceAll("from 'vue'", `from ${JSON.stringify(vue)}`);
const card = (await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`)).default;
const now = Date.now();
const target = { taskId: '0123456789abcdef01234567', name: 'fixture', isRemote: false, authProfile: 'user1', launchAttemptId: 'launch1' };
const task = { _id: target.taskId, name: target.name, isRemote: false, auth_profile: 'user1', taskType: 'regular', status: 'running', launchAttemptId: 'launch1' };
const metadata = () => ({ schemaVersion: 1, ...target, source: 'priority_capability', sourceDetail: null,
  consumerRunId: '0123456789abcdef0123456789abcdef', launchMatched: true, heartbeatAt: now, occurredAt: now - 1000,
  errorClass: 'simulation_recovery_phase_timeout', reason: 'original receipt recovery exceeded 300s; receipts retained',
  phase: 'simulation_recovery_phase', timeoutSeconds: 300, scope: 'accepted_receipts', recoveryState: 'waiting_original_recovery', nextRetryAt: now + 15000 });
const positive = '✅ Pane 0 (pid=123) is running: python3';
const negative = '❌ Pane 0 (pid=123) is running: sleep';
const marker = 'QUANTNIGHT_BUSINESS_DIAGNOSTIC ';
const normalize = (d = metadata(), pane = positive) => manager.normalizeTaskRuntime(target, `${pane}\n${marker}${JSON.stringify(d)}`, now);
async function render(observation, currentTask = task) {
  const pinia = createPinia(); setActivePinia(pinia);
  const store = useTaskStore();
  // Real Pinia remains the original control authority; diagnostics must never
  // mutate task/control status or counters, even when an error says "shutdown".
  const before = JSON.stringify(store.$state);
  const app = createSSRApp({ render: () => h(card, { task: currentTask, taskRuntimeStatus: observation,
    progress: { total: 142, success: 90, error: 12, priority_total: 0, priority_success: 0, priority_error: 0 } }) });
  app.use(pinia); app.config.warnHandler = () => {};
  const stub = { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) };
  for (const name of ['n-card','n-tag','n-text','n-space','n-switch','n-button']) app.component(name, stub);
  const html = await renderToString(app);
  assert.equal(JSON.stringify(store.$state), before);
  return html;
}

test('original SFC normalizer to real card/Pinia distinguishes current waiting from exited historical error', async () => {
  for (const [pane, label] of [[positive, '当前消费者错误 / 等待原恢复'], [negative, '最后历史业务错误（未证明退出因果）']]) {
    const observation = normalize(metadata(), pane);
    assert.equal(observation.state, pane === positive ? 'running' : 'not_running');
    const html = await render(observation);
    const summary = html.match(/<summary[^>]*>(.*?)<\/summary>/s)[1];
    assert.equal(summary, '运行 / 停止详情');
    const body = html.slice(html.indexOf('</summary>') + '</summary>'.length);
    assert.ok(body.includes(label)); assert.ok(body.includes('simulation_recovery_phase_timeout'));
    assert.ok(body.includes('original receipt recovery exceeded 300s'));
    assert.doesNotMatch(html, /<details[^>]*\sopen(?:[=>\s])/);
    for (const text of ['发生时间（UTC）','simulation_recovery_phase','300 秒','accepted_receipts','waiting_original_recovery','✓ 90','✗ 12','Σ 142']) assert.ok(html.includes(text), text);
    assert.ok(html.includes(pane === positive ? '原有下次合法重试时间' : '历史重试计划'));
  }
});

test('identity, host, profile, launch, future time and malformed errors are rejected, never replace process truth', async () => {
  for (const patch of [{ taskId:'other' },{ name:'other' },{ isRemote:true },{ authProfile:'user2' },{ launchAttemptId:'old' },
    { consumerRunId:'bad' },{ source:'stdout' },{ occurredAt:now+1 },{ heartbeatAt:now-2000 },{ scope:'all_shutdown' },
    { timeoutSeconds:0 },{ errorClass:'EOFError: Feishu' },{ recoveryState:'line\npaused' }]) {
    const observation = normalize({ ...metadata(), ...patch });
    assert.equal(observation.state, 'running'); assert.equal(observation.businessDiagnostic, null, JSON.stringify(patch));
    assert.doesNotMatch(await render(observation), /simulation_recovery_phase_timeout/);
  }
  const late = normalize();
  assert.equal(businessDiagnostic({ ...task, launchAttemptId:'new-launch' }, late, now), null);
  assert.equal(businessDiagnostic({ ...task, isRemote:true }, late, now), null);
  for (const output of ['', `${marker}${JSON.stringify(metadata())}`, `Permission denied\n${marker}${JSON.stringify(metadata())}`]) {
    const observation = manager.normalizeTaskRuntime(target, output, now);
    assert.equal(observation.state, 'unknown'); assert.equal(businessDiagnostic(task, observation, now), null);
  }
});

test('old or launch-unproven evidence stays historical; no fabricated current retry', () => {
  for (const patch of [{ heartbeatAt:now-121000, occurredAt:now-122000 },{ launchMatched:false, launchAttemptId:null }]) {
    const value = businessDiagnostic(task, normalize({ ...metadata(), ...patch }), now);
    assert.match(value.summary, /历史业务错误/); assert.match(value.detail, /历史重试计划/);
    assert.doesNotMatch(value.summary, /当前消费者错误/);
  }
  const value = businessDiagnostic(task, normalize({ ...metadata(), nextRetryAt:null }), now);
  assert.match(value.detail, /原证据未提供；诊断不安排重试/);
});

test('tmux fallback is explicitly historical and secrets/secondary EOF never become cause or process state', async () => {
  const d = { ...metadata(), source:'tmux_log', sourceDetail:'exact session $5; createdAt=1790000000000', consumerRunId:'unavailable',
    launchMatched:false, launchAttemptId:null, nextRetryAt:null, reason:'budget exceeded mongodb://user:secret@host/db?token=private; password=secret; receipt retained' };
  const observation = normalize(d, negative);
  const html = await render(observation);
  assert.match(html, /最后历史业务错误/); assert.match(html, /tmux_log/); assert.match(html, /receipt retained/);
  assert.doesNotMatch(html, /user:secret|token=private|password=secret|host\/db/);
  const fallback = manager.normalizeTaskRuntime(target, negative, now);
  assert.match(taskDiagnosticSummary(task, fallback, now), /缺少可归因/);
  for (const output of [`${negative}\n${marker}{broken}`, `${negative}\n${marker}${JSON.stringify(d)}\n${marker}${JSON.stringify(d)}`,
    `${marker}${JSON.stringify(d)}\n${negative}`, `${negative}\nEOFError: Feishu shutdown`]) {
    const bad = manager.normalizeTaskRuntime(target, output, now);
    assert.equal(bad.businessDiagnostic, null);
    assert.doesNotMatch(taskDiagnosticSummary(task, bad, now), /simulation_recovery_phase_timeout/);
  }
});

test('native retained dead pane is no consumer, keeps historical error, never completed business', async () => {
  const dead = '❌ Pane 0 (pid=123) exited (retained tmux pane)';
  const observation = normalize(metadata(), dead);
  assert.equal(observation.state, 'not_running');
  const html = await render(observation);
  assert.match(html, /未检测到运行消费者/);
  assert.match(html, /最后历史业务错误/);
  assert.doesNotMatch(html, /当前消费者错误|fixture completed/);
});


test('actual original script/fakeOS -> native original runner -> this SFC/Pinia from native test output',
  { skip: !process.env.QUANTNIGHT_NATIVE_DIAGNOSTIC_LOG }, async () => {
    const log = await readFile(process.env.QUANTNIGHT_NATIVE_DIAGNOSTIC_LOG, 'utf8');
    const lines = log.split('\n').filter(line => line.startsWith('NATIVE_DIAGNOSTIC_TEST_WIRE '));
    assert.ok(lines.length > 0, 'native real-chain test must have emitted its exact wire result');
    const fixture = JSON.parse(lines.at(-1).slice('NATIVE_DIAGNOSTIC_TEST_WIRE '.length));
    const result = manager.normalizeTaskRuntime(fixture, fixture.output, fixture.observedAt);
    assert.equal(result.state, 'not_running');
    assert.equal(result.businessDiagnostic.source, 'tmux_log');
    const html = await render(result);
    assert.match(html, /未检测到运行消费者/);
    assert.equal(html.match(/<summary[^>]*>(.*?)<\/summary>/s)[1], '运行 / 停止详情');
    assert.match(html.slice(html.indexOf('</summary>')), /simulation_recovery_phase_timeout/);
    assert.match(html, /300 秒/); assert.match(html, /最后历史业务错误/);
    assert.doesNotMatch(html, /private|EOFError|fake paused/);
  });


test('session absent card keeps long historical business error inside the closed disclosure', async () => {
  const error = { ...metadata(), errorClass: 'simulation_task_error', phase: 'simulation_task',
    reason: 'simulation_task: ValueError during operation; exception=ValueError; budget=not configured/available', timeoutSeconds: null };
  const observation = normalize(error, "❌ Session 'fixture' 不存在 (local)");
  const html = await render(observation);
  const summary = html.match(/<summary[^>]*>(.*?)<\/summary>/s)[1];
  assert.equal(summary, '运行 / 停止详情');
  assert.doesNotMatch(html, /<details[^>]*\sopen(?:[=>\s])/);
  assert.match(html.slice(html.indexOf('</summary>')), /simulation_task_error/);
  assert.match(html.slice(html.indexOf('</summary>')), /ValueError during operation/);
  assert.match(html.slice(0, html.indexOf('<details')), /已暂停/);
});
