import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const commands = read('../src-tauri/src/commands.rs');
const main = read('../src-tauri/src/main.rs');
const facade = read('../crates/task-core/src/task_control.rs');
const body = name => commands.slice(commands.indexOf(`pub async fn ${name}(`)).split('\n}')[0];

test('all active task mutations call the shared facade and never Mongo writes, taskctl Python bridge, or drop sync', () => {
  for (const [name, route] of [['create_task', '.create('], ['generate_list', '.generate('],
    ['sync_remote_task', '.sync_bound('], ['delete_task', '.delete_bound('], ['pause_task', '.pause('], ['update_priority_task', '.update_priority(']]) {
    const source = body(name);
    assert.ok(source.includes(route), `${name} -> ${route}`);
    assert.doesNotMatch(source, /update_one|insert_one|\.drop\(|run_python_command|run_bash_script|run_taskctl/);
  }
  const production = commands.slice(0, commands.indexOf('mod verified_pause_tests'));
  assert.doesNotMatch(production, /taskctl\.py|--drop|delete_remote_registered_task|archive_prepared_research|\.update_one\(|\.insert_one\(|\.drop\(/);
  assert.match(body('update_remote_status'), /Err\(format!/);
  assert.doesNotMatch(body('update_remote_status'), /MongoClients|TaskControl|update_one|isRemote.*\$set/);
});

test('one verified startup publishes shared AppConfig/pools/control before GUI reads, without button bootstrap', () => {
  assert.equal((main.match(/TaskControl::bootstrap_gui\(config\)/g) || []).length, 1);
  assert.equal((main.match(/manage\(commands::TaskControlState::from_verified\(control\)\)/g) || []).length, 1);
  assert.ok(main.indexOf('load_config(&config_path)') < main.indexOf('TaskControl::bootstrap_gui(config)'));
  assert.ok(main.indexOf('context.into_parts()') < main.indexOf('tauri::Builder'));
  assert.ok(main.indexOf('dev_identity::verify_gui_root') < main.indexOf('TaskControl::bootstrap_gui(config)'));
  assert.ok(main.indexOf('context.into_parts()') < main.indexOf('dev_identity::ready()'));
  assert.match(main.split('Application unavailable:')[1].split('tauri::Builder')[0], /std::process::exit\(1\)/);
  assert.doesNotMatch(main, /MongoClients::new|TaskControlState::default/);
  assert.match(main, /Application unavailable/);
  assert.match(main, /eprintln!\("MongoDB endpoint authority: backend MongoConfig[^"\n]+legacy TOML URI values are not used for transport[^"\n]+"\);/);
  const notice = main.split('eprintln!("MongoDB endpoint authority:')[1].split('\n')[0];
  assert.doesNotMatch(notice, /\{.*\}|mongodb:\/\/|mongodb\+srv:\/\//);
  assert.match(main, /manage\(mongo_clients.clone\(\)\)/);
  assert.match(main, /manage\(config\)/);
  const init = commands.slice(commands.indexOf('pub struct TaskControlState'), commands.indexOf('fn core_receipt'));
  assert.match(init, /OnceCell<Result<TaskControl, String>>/);
  assert.match(init, /TaskControl::with_existing_clients\(config, clients.clone\(\)\)/);
  assert.match(init, /get_or_init/);
  assert.match(init, /OnceCell::new_with\(Some\(Ok\(control\)\)\)/);
  assert.doesNotMatch(init, /get_or_try_init|bootstrap_gui|export_backend_config/);
  const binding = read('../crates/task-core/src/backend_binding.rs');
  assert.match(binding, /run_process_private\(command, Duration::from_secs\(8\)\)/);
  assert.doesNotMatch(binding, /run_process_observed|normalize|directConnection/);
  const cli = read('../crates/taskctl/src/bootstrap.rs');
  assert.match(cli, /export_backend_config\(root\)/);
  assert.doesNotMatch(cli, /process::Command|from_value|run_process_private/);
  assert.match(facade, /MongoClients::new_with\(&config, build\)/);
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
  assert.match(body('pause_task'), /expected_task_id: Some\(id\), expected_remote: Some\(is_remote\)/);
  assert.match(body('pause_task'), /StopBudget::Gui/);
  assert.match(body('generate_list'), /timeout_seconds = 900/);
  assert.match(commands, /Duration::from_secs\(300\), core.start/);
  assert.match(commands, /GuiCreatedTask as CreatedTask/);
  assert.match(commands, /pub use quantnight_task_core::pause::ConfirmedTaskStop/);
  assert.match(commands, /required_task_id/);
  assert.match(facade, /original task is not registered in this local control plane/);
  assert.match(facade, /find_task\(Host::Local, doc! \{"_id": id\}/);
});

test('view/cache and runtime inspection retain read-only semantics, not state-reconciling core status', () => {
  assert.match(body('get_all_tasks'), /fetch_remote_tasks/);
  assert.doesNotMatch(body('get_all_tasks'), /core.status|TaskControl|update_one/);
  assert.match(body('check_task_status'), /execute_task_script/);
  assert.match(body('check_task_status'), /read_runtime_evidence/);
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
  for (const [name, method] of [['sync_remote_task', 'sync_bound'], ['delete_task', 'delete_bound']]) {
    assert.match(body(name), /expected_remote: Option<bool>/);
    assert.match(body(name), /let remote = expected_remote.unwrap_or\(task.get_bool\("isRemote"\).unwrap_or\(false\)\)/);
    assert.ok(body(name).includes(`core.${method}(`));
    assert.ok(body(name).includes('Some(remote)'));
  }
  const cleanup = read('../crates/task-core/src/cleanup_sync.rs');
  assert.match(facade, /cleanup_sync::sync_with_expected_host/);
  assert.match(facade, /cleanup_sync::delete_with_expected_host/);
  assert.match(cleanup, /original execution host changed before locked sync/);
  assert.match(cleanup, /original execution host changed before locked cleanup/);
});
