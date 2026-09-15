import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const json = path => JSON.parse(readFileSync(new URL(`../${path}`, import.meta.url), 'utf8'));
const version = json('package.json').version;
const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8').match(/^version = "([^"]+)"/m)?.[1];
const cargoLock = readFileSync(new URL('../src-tauri/Cargo.lock', import.meta.url), 'utf8').match(/name = "quantnight-frontend"\nversion = "([^"]+)"/)?.[1];
for (const [source, actual] of Object.entries({tauri:json('src-tauri/tauri.conf.json').version,cargo,cargoLock,npmLock:json('package-lock.json').version,npmRoot:json('package-lock.json').packages[''].version})) {
  assert.equal(actual, version, `${source} version does not match package.json`);
}
if (process.env.GITHUB_REF_TYPE === 'tag') {
  assert.equal(process.env.GITHUB_REF_NAME, `v${version}`, 'Release tag must match the source version');
}
console.log(`Version metadata consistent: ${version}`);
