// Only known shipped/default paths are migrated; arbitrary user paths are opaque.
export const BUNDLED_QUANTNIGHT_ROOT = '/Users/wenliu/Code/python/quantnight';
const trimSlash = (path) => path.replace(/[\\/]+$/, '');

export function migrateTemplatePaths(paths = {}, workingDir = BUNDLED_QUANTNIGHT_ROOT) {
  const root = trimSlash(workingDir || BUNDLED_QUANTNIGHT_ROOT);
  const result = { ...paths };
  for (const [key, kind, legacy] of [
    ['templatePath', 'regular', 'templates'],
    ['superTemplatePath', 'super', 'super_templates'],
  ]) {
    const value = paths[key];
    const knownDefaults = [root, BUNDLED_QUANTNIGHT_ROOT].flatMap(base => [
      `${base}/${legacy}`, `${base}/templates/generated/${kind}`,
    ]);
    knownDefaults.push(key === 'templatePath' ? './templates' : './templates/super', `./templates/generated/${kind}`);
    if (!value || (typeof value === 'string' && knownDefaults.includes(trimSlash(value)))) {
      result[key] = `${root}/templates/generated/${kind}`;
    }
  }
  if (result.priorityTemplatePath == null) result.priorityTemplatePath = `${root}/refine_templates/`;
  return result;
}

// Descriptor names are logical template IDs, never "name.research".
export function templateName(filename) {
  const name = filename.trim().replace(/\.research\.json$/i, '').replace(/\.py$/i, '');
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(name)) {
    throw new Error('请输入裸模板名称（或 .py / .research.json 文件名），不能包含路径');
  }
  return name;
}
