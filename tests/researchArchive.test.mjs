import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const source = readFileSync(new URL('../src-tauri/src/commands.rs', import.meta.url), 'utf8');
test('existing delete button archives prepared dataset before mutation', () => {
  const body = source.slice(source.indexOf('pub async fn delete_task('), source.indexOf('pub async fn start_task('));
  assert.ok(body.indexOf('archive_prepared_research(') < body.indexOf('.update_one('));
  assert.match(body, /delete_filter/);
  const helper = source.slice(source.indexOf('async fn archive_prepared_research'), source.indexOf('pub async fn delete_task('));
  for (const part of ['archive-research', '--expected-task-id', '--expected-remote', 'archivePath', 'kill_on_drop(true)', 'Duration::from_secs(90)']) {
    assert.ok(helper.includes(part), part);
  }
  assert.ok(!helper.includes('.drop('));
});

test('native discovery/archive reuse catalog rather than duplicating sidecar resolution', () => {
  const helper = source.slice(source.indexOf('async fn archive_prepared_research'), source.indexOf('pub async fn delete_task('));
  assert.match(helper, /run_template_catalog\(&root, "binding", kind, Some\(template\), None\)/);
  assert.match(helper, /binding.get\("kind"\)/);
  assert.match(helper, /receipt.get\("command"\).*Some\("archive-research"\)/);
  assert.doesNotMatch(helper, /join\(format!\("\{template\}\.research\.json/);
  const discovery = source.slice(source.indexOf('pub async fn read_latest_py_file'), source.indexOf('pub async fn sync_remote_task'));
  assert.match(discovery, /task_type: Option<String>/);
  assert.match(discovery, /run_template_catalog\(root, "latest"/);
  assert.doesNotMatch(discovery, /fs::read_dir|modified\(\)/);
});
