import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const source = read('../src-tauri/src/commands.rs');
const facade = read('../crates/task-core/src/task_control.rs');
const cleanup = read('../crates/task-core/src/cleanup_sync.rs');
const runtime = read('../crates/task-core/src/operations_runtime.rs');

test('existing delete button delegates exact identity to shared safe archive before control mutation', () => {
  const body = source.slice(source.indexOf('pub async fn delete_task('), source.indexOf('fn start_request('));
  assert.match(body, /core\.task_by_id\(&id\)/);
  assert.match(body, /core\.delete_bound\(name, Some\(&id\), Some\(remote\), None, None\)/);
  for (const field of ['controlHidden', 'localDeleted', 'remoteDeleted', 'itemsPreserved', 'canonicalResultsDeleted']) assert.ok(body.includes(field));
  assert.doesNotMatch(body, /\.drop\(|\.update_one\(|archive_prepared_research|run_taskctl/);
  assert.match(facade, /cleanup_sync::delete_with_expected_host\(self\.root\(\), self\.store\.as_ref\(\), &runtime, name, expected_id, expected_remote\)/);
  const implementation = cleanup.slice(cleanup.indexOf('async fn delete_inner('));
  assert.ok(implementation.indexOf('.archive_research(') < implementation.indexOf('"delete-begin"'));
  for (const gate of ['stop_confirmation_valid', 'assert_consumer_absent', 'checked_unresolved_execution_row', 'validate_claim', 'remote_identity']) assert.ok(implementation.includes(gate));
  assert.match(cleanup, /rename_collection/);
  assert.doesNotMatch(cleanup, /\.drop\(/);
});

test('discovery remains a catalog read; shared archive uses original research owner with exact namespace and bounded receipt protocol', () => {
  const archive = runtime.slice(runtime.indexOf('async fn archive_research('), runtime.indexOf('fn start_task_metadata('));
  for (const part of ['util.dataset_mission', '--native-archive-statistics', 'configurationBinding', 'evidenceCollection', 'Duration::from_secs(90)', 'schemaVersion', 'command', 'ok', 'result']) assert.ok(archive.includes(part), part);
  assert.doesNotMatch(archive, /taskctl\.py|\.drop\(|\.research\.json/);
  const discovery = source.slice(source.indexOf('pub async fn read_latest_py_file'), source.indexOf('pub async fn sync_remote_task'));
  assert.match(discovery, /task_type: Option<String>/);
  assert.match(discovery, /run_template_catalog\(root, "latest"/);
  assert.doesNotMatch(discovery, /fs::read_dir|modified\(\)/);
});
