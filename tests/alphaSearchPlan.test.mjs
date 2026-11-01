import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src-tauri/src/datas.rs', import.meta.url), 'utf8');
const policy = readFileSync(new URL('../src-tauri/src/alpha_search_plan.rs', import.meta.url), 'utf8');
const helper = source.slice(source.indexOf('async fn measured_search_hint'), source.indexOf('pub async fn get_alpha_count'));
const page = source.slice(source.indexOf('pub async fn get_alpha_results'), source.indexOf('pub struct PnlPoint'));

test('measured plan is only considered for simple keyword and Sharpe sorting', () => {
  assert.match(helper, /simple_keyword_scope\(filter\) != Some\(true\)/);
  assert.match(helper, /sort\.get\("is.sharpe"\)/);
  assert.match(policy, /field != "is.sharpe"/);
  assert.match(policy, /"REGULAR"/);
  assert.match(page, /\.hint\(hint\)/);
});

test('metadata lookup is bounded and existing incomplete indexes are excluded', () => {
  assert.match(helper, /collection\.list_indexes/);
  assert.match(helper, /timeout\(StdDuration::from_secs\(1\)/);
  assert.match(helper, /from_secs\(60\)/);
  for (const flag of ['sparse', 'hidden', 'partial_filter_expression', 'collation']) {
    assert.ok(policy.includes(flag));
  }
  assert.doesNotMatch(helper + policy, /\.create_index|\.drop_index|\.insert_|\.update_/);
});

test('withdrawn six-second page deadline is not accidentally reinstated', () => {
  assert.doesNotMatch(page, /PAGE_SERVER_BUDGET|PAGE_WALL_BUDGET|\.max_time\(/);
  assert.doesNotMatch(page, /from_secs\(6\)|超过9秒/);
});

test('failed find evicts index snapshot without replaying the query', () => {
  assert.match(page, /entries\.remove\(&collection\.namespace\(\)\.to_string\(\)\)/);
  assert.match(page, /No automatic retry/);
});
