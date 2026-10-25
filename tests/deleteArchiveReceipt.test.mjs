// Narrow source contract for the native adapter; real archive orchestration is
// exercised by task-core/tests/cleanup_sync_routes.rs. This is not a GUI build.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const commands = readFileSync(new URL('../src-tauri/src/commands.rs', import.meta.url), 'utf8');
const body = commands.slice(commands.indexOf('pub async fn delete_task('), commands.indexOf('\nfn start_request('));

test('native delete returns validated compact archive JSON after existing proof checks', () => {
  assert.match(body, /Result<JsonValue, String>/);
  const returned = body.indexOf('Ok(serde_json::json!(');
  assert.ok(returned > body.indexOf('core_receipt(&result, "delete", name)?'));
  const checks = body.slice(0, returned);
  for (const key of ['controlHidden', 'localDeleted', 'remoteDeleted', 'itemsPreserved',
    'canonicalResultsDeleted', 'taskId', 'remote', 'unresolvedItemCount',
    'receiptAssessmentUnknownCount', 'platformRequestsCancelled', 'capacityReleased',
    'receiptPath', 'localArchive', 'remoteArchive']) {
    assert.ok(checks.includes(`"${key}"`), `validate ${key}`);
    assert.ok(body.slice(returned).includes(`"${key}"`), `return ${key}`);
  }
  assert.match(checks, /receipt\["taskId"\]\.as_str\(\) != Some\(id.as_str\(\)\)/);
  assert.match(checks, /receipt\["remote"\]\.as_bool\(\) != Some\(remote\)/);
  assert.match(checks, /receipt\["unresolvedItemCount"\]\.as_u64\(\)\.is_none\(\)/);
  assert.match(checks, /receipt\["receiptAssessmentUnknownCount"\]\.as_u64\(\)\.is_none\(\)/);
  assert.doesNotMatch(body.slice(returned), /taskConfig|submissionAttempts|progressUrl|result\.value\.clone/);
  assert.doesNotMatch(body, /Ok\(\(\)\)|\.pause\(|\.start\(|\.resume\(|cancel_simulation|release_slot|\.drop\(/);
  assert.equal((body.match(/\.delete_bound\(/g) || []).length, 1);
});
