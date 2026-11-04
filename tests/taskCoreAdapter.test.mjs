import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const commands = read('../src-tauri/src/commands.rs');
const main = read('../src-tauri/src/main.rs');
const facade = read('../crates/task-core/src/task_control.rs');
const dispatcher = read('../crates/task-core/src/task_dispatch.rs');
const sharedRuntime = read('../crates/task-core/src/runtime/mod.rs');
const queries = read('../crates/task-core/src/gui_task_queries.rs');
const diagnostics = read('../crates/task-core/src/task_diagnostics.rs');
const compact = value => value.replace(/\s+/g, '');
const body = name => commands.slice(commands.indexOf(`pub async fn ${name}(`)).split('\n}')[0];

test('all active task mutations call the shared facade and never Mongo writes, taskctl Python bridge, or drop sync', () => {
  for (const [name, route] of [['create_task', 'TaskRequest::Create('], ['generate_list', 'TaskRequest::Generate('],
    ['sync_remote_task', 'TaskRequest::Sync('], ['delete_task', 'TaskRequest::Delete('], ['pause_task', 'TaskRequest::Pause('], ['update_priority_task', 'TaskRequest::PriorityUpdate(']]) {
    const source = body(name);
    assert.ok(source.includes(route), `${name} -> ${route}`);
    assert.match(source, /execute_task\(|\.execute\(/);
    assert.ok(dispatcher.includes(route.replace('TaskRequest::','TaskRequest::')));
    assert.doesNotMatch(source, /update_one|insert_one|\.drop\(|run_python_command|run_bash_script|run_taskctl/);
  }
  const production = commands.slice(0, commands.indexOf('mod verified_pause_tests'));
  assert.doesNotMatch(production, /taskctl\.py|--drop|delete_remote_registered_task|archive_prepared_research|\.update_one\(|\.insert_one\(|\.drop\(/);
  assert.match(body('update_remote_status'), /Err\(format!/);
  assert.doesNotMatch(body('update_remote_status'), /MongoClients|TaskControl|update_one|isRemote.*\$set/);
});

test('one verified startup publishes shared AppConfig/pools/control before GUI reads, without button bootstrap', () => {
  assert.equal((main.match(/runtime::bootstrap_gui\(config\)/g) || []).length, 1);
  assert.equal((main.match(/manage\(commands::TaskControlState::from_endpoint\(endpoint\)\)/g) || []).length, 1);
  assert.ok(main.indexOf('load_config(&config_path)') < main.indexOf('runtime::bootstrap_gui(config)'));
  assert.ok(main.indexOf('runtime::bootstrap_gui(config)') < main.indexOf('tauri::Builder'));
  assert.ok(main.indexOf('dev_identity::verify_gui_root') < main.indexOf('runtime::bootstrap_gui(config)'));
  assert.ok(main.indexOf('runtime::bootstrap_gui(config)') < main.indexOf('dev_identity::ready()'));
  assert.match(main.split('Application unavailable:')[1].split('tauri::Builder')[0], /std::process::exit\(1\)/);
  assert.doesNotMatch(main, /MongoClients::new|TaskControlState::default/);
  assert.match(main, /Application unavailable/);
  assert.match(main, /eprintln!\("MongoDB endpoint authority: backend MongoConfig[^"\n]+legacy TOML URI values are not used for transport[^"\n]+"\);/);
  const notice = main.split('eprintln!("MongoDB endpoint authority:')[1].split('\n')[0];
  assert.doesNotMatch(notice, /\{.*\}|mongodb:\/\/|mongodb\+srv:\/\//);
  assert.match(main, /manage\(mongo_clients.clone\(\)\)/);
  assert.match(main, /manage\(config\)/);
  const init = commands.slice(commands.indexOf('pub struct TaskControlState'), commands.indexOf('fn core_receipt'));
  assert.match(init, /TaskControlState<T = TaskEndpoint>/);
  assert.match(init, /OnceCell<Result<T, String>>/);
  assert.match(init, /get_or_init/);
  assert.match(init, /OnceCell::new_with\(Some\(Ok\(endpoint\)\)\)/);
  assert.doesNotMatch(init, /get_or_try_init|bootstrap_gui|export_backend_config|with_existing_clients/);
  assert.match(sharedRuntime, /RuntimeSelection::for_backend\(root\)/);
  assert.match(sharedRuntime, /TaskControl::bootstrap_gui_data\(config\)/);
  const dataBootstrap = facade.slice(facade.indexOf('pub async fn bootstrap_gui_data('), facade.indexOf('/// Headless CLI bootstrap'));
  assert.doesNotMatch(dataBootstrap, /MongoTaskStore::|TaskControl \{/);
  assert.match(sharedRuntime, /RuntimeClient::connect_gui\(&selection,&settings\)/);
  assert.match(sharedRuntime, /TaskEndpoint::runtime\(client\)/);
  assert.match(sharedRuntime, /TaskEndpoint::embedded/);
  const binding = read('../crates/task-core/src/backend_binding.rs');
  assert.match(binding, /run_process_private\(command, Duration::from_secs\(8\)\)/);
  assert.doesNotMatch(binding, /run_process_observed|normalize|directConnection/);
  const cli = read('../crates/taskctl/src/bootstrap.rs');
  assert.match(cli, /export_backend_config\(root\)/);
  assert.doesNotMatch(cli, /process::Command|from_value|run_process_private/);
  assert.match(facade, /MongoClients::new_with\(&config,\s*build\)/);
  assert.match(facade, /MongoTaskStore::new\(clients.clone\(\), &config\)/);
  assert.match(facade, /binding.verify_current\(\)\?/);
  const watcher = read('../src-tauri/src/watcher.rs');
  assert.doesNotMatch(watcher.split('#[cfg(test)]')[0], /MongoClients::new|bootstrap_gui|export_backend_config/);
  assert.doesNotMatch(init, /MongoClients::new|load_config|credentials\.yaml/);
  assert.match(facade, /with_existing_clients/);
  assert.match(facade, /validate_backend_binding\(config\)/);
});

test('mutations require exact local ID, keep legacy DTO and preserve existing operation budgets', () => {
  assert.match(body('sync_remote_task'), /task_id: Option<String>/);
  assert.match(body('sync_remote_task'), /required_task_id\(task_id.as_deref\(\)\)/);
  const pauseOptions = commands.slice(commands.indexOf('fn gui_pause_options'), commands.indexOf('pub async fn pause_task'));
  assert.ok(compact(pauseOptions).includes('expected_task_id:Some(id),expected_remote:Some(remote)'));
  assert.ok(compact(pauseOptions).includes('expected_launch_attempt_id:exact,expected_launch_absent,remaining'));
  assert.ok(compact(body('pause_task')).includes('gui_pause_options(id,is_remote,expected_launch_attempt_id.as_deref(),expected_launch_absent'));
  assert.ok(compact(body('pause_task')).includes('remaining_ms,started.elapsed()'));
  assert.match(body('pause_task'), /TaskRequest::Pause\(PauseRequest/);
  assert.match(dispatcher, /control.pause\(&r.name,PauseOptions/);
  assert.match(dispatcher, /r.budget,bash/);
  assert.match(body('pause_task'), /StopBudget::Gui/);
  assert.match(body('generate_list'), /timeout_seconds = 900/);
  assert.ok(compact(commands).includes('Duration::from_secs(300),execute_task(core,TaskRequest::Start('));
  assert.match(commands, /GuiCreatedTask as CreatedTask/);
  assert.match(commands, /pub use quantnight_task_core::pause::ConfirmedTaskStop/);
  assert.match(commands, /required_task_id/);
  assert.match(facade, /original task is not registered in this local control plane/);
  assert.match(facade, /find_task\(Host::Local, doc! \{"_id": id\}/);
});

test('list delegates only local writing generation through original control; remote/cache/runtime stay observational', () => {
  const list = body('get_all_tasks');
  assert.match(list, /control: State<'_, TaskControlState>/);
  assert.match(list, /execute\(TaskRequest::GuiTasks/);
  assert.match(list, /gui_task_queries::decode_list/);
  assert.match(dispatcher, /gui_task_queries::list/);
  const projection = queries.slice(queries.indexOf('pub async fn list('), queries.indexOf('pub fn decode_list('));
  assert.match(projection, /fetch_remote_tasks/);
  assert.match(projection, /append_local_listed_task[\s\S]*control.reconcile_generation_for_list\(&observed\)/);
  assert.match(projection, /merge_remote_listed_tasks/);
  const adapter = queries.slice(queries.indexOf('async fn append_local_listed_task'), queries.indexOf('/// 远程任务列表缓存'));
  assert.match(adapter, /task.status == "generating"/);
  assert.match(adapter, /generation.get_str\("phase"\)[\s\S]*Some\("writing"\)/);
  assert.match(adapter, /configured_mission_database != GUI_TASK_MISSION_DATABASE/);
  assert.match(adapter, /reconcile\(document\).await/);
  assert.match(adapter, /generation_refresh_error = Some\(error\)/);
  assert.doesNotMatch(list + projection + adapter, /core\.status|\bTaskControl::|update_one|insert_one|MongoClients::new|TaskControl::connect|run_bash_script|check_task_status|\.(?:generate|start|pause|sync)\(/);
  const remote = queries.slice(queries.indexOf('async fn fetch_remote_tasks'), queries.indexOf('pub async fn list('));
  assert.doesNotMatch(remote, /reconcile_generation|append_local_listed_task|control.get/);
  assert.match(compact(remote), /remote_uri==context.config\(\).mongodb.local_uri/);
  assert.match(remote, /REMOTE_TASK_CACHE_TTL/);
  assert.match(remote, /timeout\(REMOTE_TASK_TIMEOUT/);
  assert.match(queries, /REMOTE_TASK_CACHE_TTL: Duration = Duration::from_secs\(10\)/);
  assert.match(queries, /REMOTE_TASK_TIMEOUT: Duration = Duration::from_secs\(2\)/);
  assert.ok(compact(queries).includes('skip_deserializing,skip_serializing_if="Option::is_none",rename="generationRefreshError"'));
  assert.match(body('check_task_status'), /TaskRequest::TaskDiagnostics/);
  assert.match(diagnostics, /execute_task_script/);
  assert.match(diagnostics, /read_runtime_evidence/);
  assert.doesNotMatch(body('check_task_status'), /core.status|update_one|\.pause\(/);

});

test('start forwards captured actual host to locked core, preserving destination intent without GUI eligibility rules', () => {
  const request = commands.slice(commands.indexOf('fn start_request('), commands.indexOf('fn start_receipt('));
  assert.match(request, /request.expected_remote = Some\(expected_remote.unwrap_or\(remote\)\)/);
  const start = read('../crates/task-core/src/start.rs');
  assert.match(start, /request.expected_remote.is_some_and\(\|expected\| expected != actual_remote\)/);
  assert.match(start, /if remote != actual_remote/);
  assert.match(start, /pristine_priority_target\(&task, remote\)/);
  assert.match(start, /start cannot migrate execution host/);
  assert.ok(start.indexOf('expected != actual_remote') < start.indexOf('let mut counts:', start.indexOf('async fn start_locked')));
  const manager = read('../src/views/TaskManager.vue');
  assert.match(manager, /expectedRemote: target.isRemote/);
  assert.match(manager, /original.targetRemote/);
  assert.doesNotMatch(manager, /pristine_priority_target|launchHistory|never_started_inventory/);
});

test('sync and archive send captured host into shared lock, not merely a pre-read GUI check', () => {
  for (const [name, method, variant] of [['sync_remote_task', 'sync_bound', 'Sync'], ['delete_task', 'delete_bound', 'Delete']]) {
    assert.match(body(name), /expected_remote: Option<bool>/);
    assert.match(body(name), /let remote = expected_remote.unwrap_or\(task.get_bool\("isRemote"\).unwrap_or\(false\)\)/);
    assert.ok(body(name).includes(`TaskRequest::${variant}(`));
    assert.ok(dispatcher.includes(`control.${method}(`));
    assert.ok(body(name).includes('Some(remote)'));
  }
  const cleanup = read('../crates/task-core/src/cleanup_sync.rs');
  assert.match(facade, /cleanup_sync::sync_with_expected_host/);
  assert.match(facade, /cleanup_sync::delete_with_expected_host/);
  assert.match(cleanup, /original execution host changed before locked sync/);
  assert.match(cleanup, /original execution host changed before locked cleanup/);
});

test('read-only dependency guard distinguishes source metadata enum from control facade', () => {
  const forbidden = /core.status|\bTaskControl\b|update_one/;
  assert.doesNotMatch('listed_task(document, TaskControlSource::Local)', forbidden);
  for (const mutation of ['TaskControl::new()', 'core.status(request)', 'collection.update_one(query, update)']) {
    assert.match(mutation, forbidden);
  }
});


test('generation-list route guard rejects generic status, process, mutation and new-pool bypasses', () => {
  const forbidden = /core\.status|\bTaskControl::|update_one|insert_one|MongoClients::new|run_bash_script|check_task_status|\.(?:generate|start|pause|sync)\(/;
  assert.doesNotMatch('core.reconcile_generation_for_list(&observed).await', forbidden);
  for (const bad of ['core.status(request)', 'collection.update_one(query, update)', 'core.generate(request)',
    'core.start(request)', 'core.pause(request)', 'core.sync(request)', 'MongoClients::new(config)',
    'TaskControl::connect(config)', 'check_task_status(task)', 'run_bash_script(script)']) assert.match(bad, forbidden);
});
