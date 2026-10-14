// Pure configuration helpers shared by the create/start/update dialogs.
// Keep validation aligned with refine_templates/pipeline.py and rounds.py.
export const PRIORITY_TEMPLATES = Object.freeze([
  "ablation", "turnover_adjust", "operator_transfer", "group_op_add", "event_op_add",
  "datafield_replacement", "change_settings", "fastexpr_to_python", "sign_flip", // legacy-only pure flip
]);
export const DEFAULT_PRIORITY_TEMPLATES = Object.freeze(PRIORITY_TEMPLATES.slice(0, 5));
export const DEFAULT_PRIORITY_ROUNDS = Object.freeze({
  enabled: true, max_rounds: 3, max_simulations_per_round: 60, promote_top_k: 5,
  exploration_fraction: 0.2, wait_timeout: 7200, poll_interval: 30,
});
export const PRIORITY_CONFIG_HINT = 'refine_templates: "auto" 9种模板中默认启用前5种 ablation、turnover_adjust、operator_transfer、group_op_add、event_op_add；新建 pipeline/auto 默认 rounds.enabled:true，每个 root 独立最多3阶段×每阶段60次实际回测，每个 root 累计最多180次，池无全局60/180预算或阶段屏障；兼容字段 rounds.max_rounds 表示每root最大阶段数，max_simulations_per_round 表示每root阶段cap；同一 root 每阶段等实际反馈后晋级前5名，再按已测表现与20%探索自适应选择方向，不按配置列表顺序分配预算。sign 方向规范化为免费前置，不单独回测纯翻转；独立 ablation 只消融结构、绝不改变 settings，实际回测占预算；turnover_adjust 仅产生0/1条 decay-only 变体、无消融。sign_flip 登记在最后，仅供legacy纯翻转；rounds.enabled:true 或 direction_normalization:true 时显式启用 sign_flip 会报错：sign flip是免费前置，请用ablation/auto，不会静默迁移；旧配置需明确改模板名，不重置预算。每模板默认10条，作用于每源×方向的候选池；max_variants_per_alpha 默认40只约束legacy扇出，不是root阶段回测预算。source_scope 默认 active，持续吸收所选 Priority 主机 control 注册表中同 auth 的当前 active regular 任务的成功真实 results；可选 source_tasks 非空任务名列表仅作筛选，不指定固定 source_task，不依赖 tags。root 的 source_task/source_remote 一经建立不可变，切换活跃任务只影响新 roots；变体写回该 root 原来源任务。前端仅启动和监控；per-root controller/CPU生成在所选 Priority 运行主机执行（可remote/local），CP目标独立于来源执行，来源按root.source_remote路由；原任务 normal consumer 须支持 protocol3；未升级则等待，禁止启动 standalone Consumer 抢槽。旧 campaign/UNKNOWN/wrong-route 行不自动迁移或重新派发。省略 rounds 或 enabled:false 保持 legacy continuous 与列表顺序；旧保存配置不注入 rounds。可改为模板名数组或 [{"name":"event_op_add","enabled":true,"params":{"num_variants":6,"max_attempts":60},"max_variants":10}]，enabled:false 禁用该模板，[] 全部关闭。event_op_add 的 open_events / exit_events 可设为非空字符串列表，num_variants 默认6、max_attempts 默认60（均须正整数），include_regional 须为布尔值。datafield_replacement / change_settings / fastexpr_to_python 仅显式加入时启用。';
const RESERVED_PARAMS = new Set(["response", "async_client", "client", "auth_profile", "task_name", "force", "ablation_only"]);

const clone = (value) => JSON.parse(JSON.stringify(value));
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

export function parseTaskConfig(value) {
  const config = typeof value === "string" ? JSON.parse(value) : value;
  if (!isObject(config)) throw new Error("配置必须是 JSON 对象，不能是数组或 null");
  return clone(config);
}

function positiveInt(value, name) {
  if (!Number.isSafeInteger(value) || value < 1) throw new Error(`${name} 必须为正整数`);
}

export function validatePriorityConfig(value) {
  const config = parseTaskConfig(value);
  if (Object.hasOwn(config, "source_scope") && config.source_scope !== "active") {
    throw new Error('source_scope 仅支持 active（所选 Priority 主机 control 同 auth 的活跃 regular 来源）');
  }
  if (Object.hasOwn(config, "source_tasks") && (!Array.isArray(config.source_tasks) ||
      config.source_tasks.length === 0 || config.source_tasks.some((name) => typeof name !== "string" || !name.trim()))) {
    throw new Error("source_tasks 必须为非空字符串列表；省略该字段表示动态 active 来源");
  }
  // Validate rounds before the legacy refine_templates early return.
  if (config.rounds != null) {
    const rounds = config.rounds;
    if (!isObject(rounds) || Object.keys(rounds).some((key) => !Object.hasOwn(DEFAULT_PRIORITY_ROUNDS, key))) {
      throw new Error("rounds 仅支持 enabled / max_rounds / max_simulations_per_round / promote_top_k / exploration_fraction / wait_timeout / poll_interval");
    }
    if (rounds.enabled !== undefined && typeof rounds.enabled !== "boolean") throw new Error("rounds.enabled 必须为布尔值");
    for (const key of ["max_rounds", "max_simulations_per_round", "promote_top_k"]) {
      if (Object.hasOwn(rounds, key)) positiveInt(rounds[key], `rounds.${key}`);
    }
    if (rounds.max_rounds > 3) throw new Error("rounds.max_rounds 上限为3");
    if (rounds.max_simulations_per_round > 60) throw new Error("rounds.max_simulations_per_round 上限为60");
    for (const key of ["exploration_fraction", "wait_timeout", "poll_interval"]) {
      if (!Object.hasOwn(rounds, key)) continue;
      const number = rounds[key];
      if (typeof number !== "number" || !Number.isFinite(number) ||
          (key === "exploration_fraction" ? number < 0 || number > 1 : number <= 0)) {
        throw new Error(`rounds.${key} 必须为有限数值，探索比例范围0..1，等待/轮询时间须大于0`);
      }
    }
  }
  if (config.max_variants_per_alpha !== undefined) {
    positiveInt(config.max_variants_per_alpha, "max_variants_per_alpha");
  }
  if (config.direction_normalization !== undefined && typeof config.direction_normalization !== "boolean") {
    throw new Error("direction_normalization 必须为布尔值");
  }
  const directionMode = config.rounds?.enabled === true || config.direction_normalization === true;
  const raw = config.refine_templates;
  // Only missing retains legacy fallback; null is an invalid explicit value.
  if (raw === undefined) return config;
  if (raw === null) throw new Error("refine_templates 不能为 null；缺省请删除该字段");
  const steps = typeof raw === "string"
    ? (raw === "auto" ? DEFAULT_PRIORITY_TEMPLATES : raw.split(","))
    : raw;
  if (!Array.isArray(steps)) throw new Error("refine_templates 必须为 auto、逗号分隔字符串或数组");
  const seen = new Set();
  for (const item of steps) {
    const step = typeof item === "string" ? { name: item.trim() } : item;
    if (!isObject(step) || Object.keys(step).some((key) => !["name", "enabled", "params", "max_variants"].includes(key))) {
      throw new Error("模板仅支持 name / enabled / params / max_variants");
    }
    if (!PRIORITY_TEMPLATES.includes(step.name)) throw new Error(`未知 refine 模板: ${step.name}`);
    if (seen.has(step.name)) throw new Error(`模板重复: ${step.name}`);
    seen.add(step.name);
    if (step.enabled !== undefined && typeof step.enabled !== "boolean") throw new Error(`${step.name}.enabled 必须为布尔值`);
    if (directionMode && step.name === "sign_flip" && step.enabled !== false) {
      throw new Error("sign flip是免费前置，请用ablation/auto");
    }
    const directionReserved = directionMode ? ["direction_normalization"] : [];
    if (step.params !== undefined && (!isObject(step.params) || Object.keys(step.params).some((key) => RESERVED_PARAMS.has(key) || directionReserved.includes(key)))) {
      throw new Error(`${step.name}.params 必须为对象，且不能覆盖 runner 的身份/内部参数`);
    }
    if (step.max_variants !== undefined) positiveInt(step.max_variants, `${step.name}.max_variants`);
    const budgetParams = {
      datafield_replacement: ["max_combos"],
      change_settings: ["max_variants", "max_attempts"],
      group_op_add: ["num_variants"],
      event_op_add: ["num_variants", "max_attempts"],
    }[step.name] || [];
    for (const key of budgetParams) {
      if (step.params?.[key] !== undefined) positiveInt(step.params[key], `${step.name}.params.${key}`);
    }
    if (step.name === "event_op_add") {
      for (const key of ["open_events", "exit_events"]) {
        const events = step.params?.[key];
        if (events !== undefined && (!Array.isArray(events) || events.length === 0 || events.some((event) => typeof event !== "string" || !event.trim()))) {
          throw new Error(`${step.name}.params.${key} 必须为非空字符串列表`);
        }
      }
      if (step.params?.include_regional !== undefined && typeof step.params.include_regional !== "boolean") {
        throw new Error(`${step.name}.params.include_regional 必须为布尔值`);
      }
    }
  }
  return config;
}

export function priorityDefaultConfig(task = { templatefile: "pipeline" }) {
  const config = {
    interval: 600,
    priority: 1,
    filter_params: { sharpe_th: 1.2, fitness_th: 0.8, usage: "check" },
  };
  const template = task.templatefile || (task.name || "").replace(/__priority$/, "");
  if (["pipeline", "auto"].includes(template)) {
    Object.assign(config, { source_scope: "active", mission_tag: "pipeline", refine_templates: "auto", max_variants_per_alpha: 40,
      rounds: { ...DEFAULT_PRIORITY_ROUNDS } });
  }
  return config;
}

export function resolveTaskConfig(task, defaults) {
  // Presence, not truthiness: {} and refine_templates:[] must never expand.
  for (const key of ["mission_config", "startConfig", "config"]) {
    if (task?.[key] !== undefined && task[key] !== null) return parseTaskConfig(task[key]);
  }
  return parseTaskConfig(defaults);
}

export function priorityTaskPayload(name, authProfile, configText, isRemote = true) {
  if (typeof isRemote !== "boolean") throw new Error("运行主机必须为布尔值");
  const taskName = name.trim();
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]{0,99}$/.test(taskName)) throw new Error("任务名须以字母或数字开头，最多100字符，仅支持字母、数字、下划线和连字符");
  const config = validatePriorityConfig(configText);
  // A newly created pipeline task must not fall back to importing its custom name,
  // even if the user replaced the JSON with {}. Existing tasks are never injected.
  if (config.mission_tag == null) config.mission_tag = "pipeline";
  return {
    name: taskName,
    template: "",
    templatefile: "pipeline",
    mission_config: config,
    auth_profile: authProfile.trim() || "user1",
    status: "pending",
    is_remote: isRemote,
  };
}
