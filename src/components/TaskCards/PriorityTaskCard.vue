<template>
  <n-card
    embedded
    size="small"
    :segmented="true"
    class="task-card priority-task"
    style="margin: auto"
  >
    <template #header>
      <div class="card-header">
        <div class="progress-background" :style="progressBarStyle" />
        <div class="header-content">
          <span class="task-name">{{ task.name }}</span>
          <n-tag class="runtime-badge" :type="getStatusType(displayStatus)" size="small" round>
            {{ displayStatus }}
          </n-tag>
        </div>
      </div>
    </template>

    <details class="operation-diagnostics" style="font-size: 12px; margin-bottom: 4px">
      <summary style="cursor: pointer">运行 / 停止详情</summary>
      <div style="white-space: pre-wrap; overflow-wrap: anywhere">
        <div>消费者检查：{{ consumerStatus }}</div>
        <div v-if="diagnosticDetails">{{ diagnosticDetails }}</div>
      </div>
    </details>
    <div v-if="displayProgress" style="margin-bottom: 4px">
      <div style="font-size: 12px; margin-bottom: 2px">
        <n-text type="success" title="成功">
          ✓ {{ displayProgress.success }}
        </n-text>
        &nbsp;|&nbsp;
        <n-text type="error" title="失败">
          ✗ {{ displayProgress.error }}
        </n-text>
        &nbsp;|&nbsp;
        <n-text depth="3" :title="roundEnabled || runtime ? '已保留计划总数' : '总计'">
          Σ {{ displayProgress.total }}
        </n-text>
      </div>
    </div>
    <div v-else style="font-size: 12px; margin-bottom: 4px">
      <n-text depth="3">✓ — | ✗ — | Σ —（进度未确认）</n-text>
    </div>
    <div v-if="progressStale" style="font-size: 12px; margin-bottom: 4px">
      <n-text type="warning">{{ progressError || '进度加载中，尚未收到实际计数…' }}</n-text>
      <n-text v-if="displayProgress" depth="3">（保留上次实际统计）</n-text>
    </div>
    <div>{{ stopStatus || hasProcessDiagnostic || !(roundEnabled || runtime) ? '任务状态' : '研究状态' }}：{{ displayRuntimeStatus }}</div>
    <div v-if="(stopStatus || hasProcessDiagnostic) && (roundEnabled || runtime)">
      研究状态：{{ researchStatus }}
    </div>
    <div v-if="roundEnabled || runtime" style="font-size: 12px; margin-top: 4px">
      <template v-if="!legacyRuntime">
        <div v-for="group in sourceGroups" :key="group.key" class="source-group">
          {{ group.source_task }}<template v-if="group.distinguishHost">（{{ group.source_remote ? '远程' : '本地' }}）</template> · Roots {{ group.root_count ?? '—' }} ·
          R0 {{ group.stage_distribution ? (group.stage_distribution['0'] ?? 0) : '—' }} /
          R1 {{ group.stage_distribution?.['1'] ?? '—' }} /
          R2 {{ group.stage_distribution?.['2'] ?? '—' }} /
          R3 {{ group.stage_distribution?.['3'] ?? '—' }}
        </div>
        <div v-if="!sourceGroups.length">Roots {{ runtime?.root_count ?? '—' }} ·
          R0 {{ runtime?.stage_distribution ? (runtime.stage_distribution['0'] ?? 0) : '—' }} /
          R1 {{ runtime?.stage_distribution?.['1'] ?? '—' }} /
          R2 {{ runtime?.stage_distribution?.['2'] ?? '—' }} /
          R3 {{ runtime?.stage_distribution?.['3'] ?? '—' }}</div>
        <div v-if="sourceGroupsUnconfirmed"><n-text type="warning">来源分组未确认，请更新状态</n-text></div>
      </template>
      <n-text v-else type="warning">旧 campaign 记录（只读）</n-text>
      <div v-if="legacyRuntime || !sourceGroups.length">来源任务：{{ sourceNames }}</div>
      <div v-if="runtimeWarning"><n-text type="warning">{{ runtimeWarning }}</n-text></div>
      <div v-if="runtime?.data_source === 'own_task_fallback'"><n-text type="warning">仅旧本任务集合（账本未确认）</n-text></div>
      <div v-if="runtime?.reason === 'source_consumer_upgrade_required'"><n-text type="warning">
        等待来源任务 normal consumer 的 protocol3 支持
      </n-text></div>
      <details class="runtime-details">
        <summary>详情 / Details</summary>
        <template v-if="!legacyRuntime">
          <div v-if="sourceGroups.length">总 Roots {{ runtime?.root_count ?? '—' }} ·
            R0 {{ runtime?.stage_distribution ? (runtime.stage_distribution['0'] ?? 0) : '—' }} /
            R1 {{ runtime?.stage_distribution?.['1'] ?? '—' }} /
            R2 {{ runtime?.stage_distribution?.['2'] ?? '—' }} /
            R3 {{ runtime?.stage_distribution?.['3'] ?? '—' }}</div>
          <div v-for="group in sourceGroups" :key="'host:' + group.key">来源执行：{{ group.source_task }} ·
            {{ typeof group.source_remote === 'boolean' ? (group.source_remote ? 'remote' : 'local') : '主机未确认' }}</div>
          <div>每 root ≤3阶段 · ≤60/阶段 · ≤180累计</div>
          <div>各 root 使用冻结配置中的阶段上限；Priority运行主机可local/remote；来源执行独立按source_remote，可local/remote</div>
          <div>待规划 {{ runtime?.stage_distribution?.['0'] ?? '—' }} · 待反馈 {{ runtime?.waiting_feedback ?? '—' }} · 待slots {{ runtime?.waiting_slots ?? '—' }} ·
            完成roots {{ runtime?.completed ?? '—' }} · 阻塞roots {{ runtime?.blocked ?? '—' }}</div>
        </template>
        <div>来源选择：动态 active regular（所选Priority主机control同 auth，来源可local/remote）；指定来源以来源任务列表为准</div>
        <div v-if="runtime?.reason">状态说明：{{ runtime.reason }}</div>
        <div>待完成 {{ runtime?.unresolved ?? '—' }}（含等待与待核验结果）</div>
        <div v-if="priorityConfig?.filter_params">Filter Params: {{ priorityConfig.filter_params }}</div>
      </details>
    </div>
    <div style="margin-top: 4px; font-size: 12px; color: #555">
      <span :title="PRIORITY_SCHEDULING_HINT">调度优先级 {{ priorityConfig?.priority ?? 1 }}</span>
    </div>

    <n-space align="center" style="margin-top: 2px">
      <n-switch
        :value="task.isRemote"
        @update:value="(val) => $emit('update:isRemote', val)"
        :checked-value="true"
        :unchecked-value="false"
      >
        <template #checked>远程</template>
        <template #unchecked>本地</template>
      </n-switch>
      <n-tag v-if="task.auth_profile" size="small" :bordered="false" type="info" style="background-color: #e3f2fd; color: #1976d2;">
        {{ task.auth_profile }}
      </n-tag>
      <n-tag v-else size="small" :bordered="false" type="info" style="background-color: #e3f2fd; color: #1976d2;">
        user1
      </n-tag>
    </n-space>
    <n-space style="margin-top: 10px" justify="center">
      <n-button size="small" type="success" @click="$emit('start')">
        <template #icon>>></template>启动
      </n-button>
      <n-button size="small" @click="$emit('update')">
        <template #icon>↻</template>更新
      </n-button>
      <n-button size="small" type="warning" @click="$emit('pause')">
        <template #icon>■</template>停止
      </n-button>
      <n-button size="small" type="error" @click="$emit('delete')">
        <template #icon>x</template>删除
      </n-button>
    </n-space>
  </n-card>
</template>

<script>
// Pure event boundary; a different task/source/campaign cannot reuse statistics.
export function acceptPriorityRuntime(event, task, previous) {
  const id = typeof task?._id === 'object' ? task._id?.$oid : task?._id;
  return event?.collection === task?.name && event.task_id === id &&
    event.is_remote === (task.isRemote === true) &&
    Number.isSafeInteger(event.updated_at) && event.updated_at >= 0 &&
    (!Number.isSafeInteger(previous?.updated_at) || event.updated_at >= previous.updated_at);
}

export function mergePriorityRuntime(event, previous) {
  const sameIdentity = previous?.task_id === event.task_id && previous?.is_remote === event.is_remote;
  // A metadata transport failure doesn't establish a new campaign. Keep the
  // last confirmed campaign/stage/counts visibly stale until a successful read.
  const incoming = sameIdentity && event.reason !== 'root_pool_not_registered' &&
    event.last_error && !event.campaign_id && !event.pool_id
    ? { ...previous, ...event, campaign_id: previous.campaign_id, pool_id: previous.pool_id,
        mode: previous.mode, stage_distribution: previous.stage_distribution, root_count: previous.root_count,
        waiting_feedback: previous.waiting_feedback, waiting_slots: previous.waiting_slots,
        completed: previous.completed, blocked: previous.blocked, source_tasks: previous.source_tasks,
        source_summaries: previous.source_summaries,
        stage: previous.stage, status: previous.status, reason: previous.reason,
        current_round: previous.current_round, max_rounds: previous.max_rounds,
        max_per_round: previous.max_per_round, unresolved: previous.unresolved }
    : event;
  const same = sameIdentity && previous?.pool_id === incoming.pool_id &&
    previous?.campaign_id === incoming.campaign_id && previous?.mode === incoming.mode;
  const counts = incoming.stats_confirmed ? incoming.counts : (same ? previous?.counts : null);
  return { ...incoming, counts: counts ?? null };
}

// Groups belong to source task + logical source target, not the Priority host.
// Old single-source metadata can use the aggregate; never copy it to multiple sources.
export function prioritySourceGroups(runtime, configuredSources) {
  const names = [...new Set(runtime?.source_tasks?.length ? runtime.source_tasks
    : (Array.isArray(configuredSources) ? configuredSources : []))];
  const summaries = runtime?.source_summaries;
  const groups = Array.isArray(summaries) ? summaries.map(group => ({ ...group })) : [];
  if (!groups.length) {
    const singleFallback = names.length === 1 && summaries == null;
    const emptyPool = Array.isArray(summaries) && runtime?.root_count === 0;
    return names.map(source_task => ({ key: source_task, source_task, source_remote: null,
      root_count: emptyPool ? 0 : singleFallback ? runtime?.root_count : null,
      stage_distribution: emptyPool ? { '0': 0, '1': 0, '2': 0, '3': 0 }
        : singleFallback ? runtime?.stage_distribution : null }));
  }
  for (const source_task of names) {
    if (!groups.some(group => group.source_task === source_task)) {
      groups.push({ source_task, source_remote: null, root_count: null, stage_distribution: null });
    }
  }
  return groups.map(group => ({ ...group, key: JSON.stringify([group.source_task, group.source_remote]),
    distinguishHost: groups.filter(other => other.source_task === group.source_task).length > 1 }));
}

export function priorityDisplayStatus(runtime, roundEnabled, controlStatus, stopStatus) {
  // Research events cannot confirm process exit or replace an operation result.
  if (stopStatus) return stopStatus;
  if (controlStatus === 'paused') return '暂停标记无有效停止凭据';
  const status = runtime?.status || runtime?.stage || (roundEnabled ? 'waiting' : controlStatus);
  if (status === 'paused') return '暂停标记无有效停止凭据';
  if (runtime && ['completed', 'done', 'success'].includes(status) &&
      (!runtime.counts || runtime.unresolved !== 0)) return 'waiting';
  return String(status).toLowerCase() === 'unknown' ? `研究状态缺少有效结果：${runtime?.reason || runtime?.last_error || '缺少阶段及原因，请刷新 root 池元数据'}` : status;
}

// unresolved includes ordinary pending/processing and cannot identify UNKNOWN.
// Only explicit abnormal metadata is promoted out of the collapsed details.
export function priorityRuntimeWarning(runtime) {
  const status = String(runtime?.status || runtime?.stage || '').toLowerCase();
  const abnormal = ['blocked', 'error', 'failed', 'unknown'].includes(status) || runtime?.blocked > 0;
  const routineReasons = ['awaiting_consumer', 'no_eligible_source_results', 'no_eligible_real_source_results',
    'round_limit_reached', 'root_stage_limit_reached', 'no_eligible_successful_feedback',
    'no_improvement', 'no_new_legal_variants'];
  if (runtime?.reason && (abnormal || !routineReasons.includes(runtime.reason))) return runtime.reason;
  if (status === 'unknown') return runtime?.last_error || '研究元数据缺少有效结果及失败原因；请刷新 root 池元数据';
  if (runtime?.blocked > 0) return `阻塞roots ${runtime.blocked}：请检查来源反馈并更新状态`;
  return null;
}

export function priorityStatusType(status) {
  if (['completed', 'done', 'success'].includes(status)) return 'success';
  if (['stopping', '停止执行中', '停止确认超时', '停止回执核验中断', '暂停标记无有效停止凭据', 'blocked', 'waiting', 'waiting_feedback', 'waiting_slots', 'dispatching', 'running', 'initializing', 'idle', 'active'].includes(status)) return 'warning';
  if (['error', 'failed', '停止执行失败', '停止回执校验失败'].includes(status)) return 'error';
  return 'default';
}
</script>

<script setup>
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { PRIORITY_SCHEDULING_HINT } from '../../utils/taskConfig.js';
import { RUNTIME_MAX_AGE, regularDisplayStatus, taskDiagnosticDetails, nextRuntimeExpiry } from '../../utils/taskDiagnostics.js';
import { taskStopDisplayStatus, taskStopProofIssue } from '../../stores/taskStore.js';

const props = defineProps({
  task: {
    type: Object,
    required: true,
  },
  taskRuntimeStatus: {
    type: Object,
    default: null,
  },
  progress: {
    type: Object,
    default: () => ({}),
  },
  stopOperation: {
    type: Object,
    default: null,
  },
});

defineEmits(['update:isRemote', 'start', 'update', 'pause', 'delete']);

const runtime = ref(null);
watch(() => JSON.stringify([typeof props.task._id === 'object' ? props.task._id?.$oid : props.task._id,
  props.task.name, props.task.isRemote === true]), () => { runtime.value = null; });
// Cached metadata makes remounts truthful before the next background snapshot.
watch(() => props.progress?.priority_campaign, (payload) => {
  if (payload && acceptPriorityRuntime(payload, props.task, runtime.value)) {
    runtime.value = mergePriorityRuntime(payload, runtime.value);
  }
}, { immediate: true });
const legacyRuntime = computed(() => runtime.value?.mode === 'legacy' || (!runtime.value?.pool_id && !!runtime.value?.campaign_id));
const sourceNames = computed(() => {
  const names = runtime.value?.source_tasks?.length ? runtime.value.source_tasks : priorityConfig.value?.source_tasks;
  return Array.isArray(names) && names.length ? names.join(', ') : '动态 active regular';
});
const sourceGroups = computed(() => prioritySourceGroups(runtime.value, priorityConfig.value?.source_tasks));
const sourceGroupsUnconfirmed = computed(() => sourceGroups.value.length > 0 &&
  sourceGroups.value.some(group => group.root_count == null));
const runtimeWarning = computed(() => priorityRuntimeWarning(runtime.value));
const roundEnabled = computed(() => priorityConfig.value?.rounds?.enabled === true);
const displayProgress = computed(() => {
  if (roundEnabled.value || runtime.value) return runtime.value?.counts ?? null;
  const p = props.progress;
  return p && (!p.stale || p.total > 0) ? p : null;
});
const progressStale = computed(() => props.progress?.stale || runtime.value?.stale || !displayProgress.value);
const progressError = computed(() => runtime.value?.last_error || props.progress?.lastError);
const stopStatus = computed(() => taskStopDisplayStatus(props.task, props.stopOperation));
const runtimeNow = ref(Date.now());
let expiryTimer;
const refreshRuntimeExpiry = () => {
  clearTimeout(expiryTimer);
  runtimeNow.value = Date.now();
  if (disposed || !props.taskRuntimeStatus) return;
  const remaining = nextRuntimeExpiry(props.task, props.taskRuntimeStatus, runtimeNow.value) - runtimeNow.value;
  if (remaining > 0 && remaining <= RUNTIME_MAX_AGE) expiryTimer = setTimeout(refreshRuntimeExpiry, remaining);
};
const stopExpiryWatch = watch(() => props.taskRuntimeStatus, refreshRuntimeExpiry, { deep: true });
const consumerStatus = computed(() => regularDisplayStatus(props.task, props.taskRuntimeStatus, stopStatus.value, runtimeNow.value));
const diagnosticDetails = computed(() => taskDiagnosticDetails(props.task, props.taskRuntimeStatus, props.stopOperation, stopStatus.value, runtimeNow.value, taskStopProofIssue(props.task)));
const researchStatus = computed(() => priorityDisplayStatus(runtime.value, roundEnabled.value, props.task.status));
// A research snapshot cannot mask the current operation's actual process check.
const hasProcessDiagnostic = computed(() => props.task.status === 'running' && props.taskRuntimeStatus &&
  typeof props.taskRuntimeStatus === 'object' &&
  (props.taskRuntimeStatus.state !== 'running' || /检查/.test(consumerStatus.value)));
const displayStatus = computed(() => stopStatus.value || hasProcessDiagnostic.value ? consumerStatus.value
  : (roundEnabled.value || runtime.value) ? researchStatus.value : consumerStatus.value);
const displayRuntimeStatus = computed(() => !stopStatus.value && !hasProcessDiagnostic.value && (roundEnabled.value || runtime.value) &&
  !runtime.value?.status && !runtime.value?.stage
    ? `尚未取得 root 池状态：${runtime.value?.last_error || '元数据检查尚未返回有效阶段或研究结果'}` : displayStatus.value);

let disposed = false;
let unlisten;
onMounted(async () => {
  refreshRuntimeExpiry();
  try {
    const cleanup = await listen('task-priority-runtime-update', ({ payload }) => {
      if (!disposed && acceptPriorityRuntime(payload, props.task, {
        updated_at: Math.max(runtime.value?.updated_at ?? 0, props.progress?.updated_at ?? 0),
      })) {
        runtime.value = mergePriorityRuntime(payload, runtime.value);
      }
    });
    if (disposed) { cleanup(); return; }
    unlisten = cleanup;
    // A card mounted after the global snapshot needs its own metadata refresh.
    await invoke('refresh_task_progress');
  } catch (error) {
    if (!disposed) runtime.value = { ...runtime.value, stale: true, last_error: `Root池状态加载失败：${String(error)}` };
  }
});
onUnmounted(() => { disposed = true; clearTimeout(expiryTimer); stopExpiryWatch(); unlisten?.(); });

const progressBarStyle = computed(() => {
  const p = displayProgress.value || {};
  const success = typeof p.success === "number" ? p.success : 0;
  const error = typeof p.error === "number" ? p.error : 0;
  const total = typeof p.total === "number" && p.total > 0 ? p.total : 1;

  const greenPercent = (success / total) * 100;
  const redPercent = (error / total) * 100;

  const mainColor = "#ffb74d"; // Orange for priority success
  const errorColor = "#ff5252"; // Red for error
  const bgColor = "#fffaf2"; // Light orange background

  const layers = [];
  const sizes = [];
  const positions = [];

  layers.push(`linear-gradient(to right, ${mainColor}, ${mainColor})`);
  sizes.push(`${greenPercent}% 100%`);
  positions.push(`left top`);

  layers.push(`linear-gradient(to right, ${errorColor}, ${errorColor})`);
  sizes.push(`${redPercent}% 100%`);
  positions.push(`${greenPercent}% 0`);

  return {
    position: "absolute",
    top: "0",
    left: "0",
    height: "100%",
    width: "100%",
    zIndex: 0,
    borderRadius: "10px 10px 0 0",
    overflow: "hidden",
    pointerEvents: "none",
    backgroundColor: bgColor,
    backgroundImage: layers.join(", "),
    backgroundSize: sizes.join(", "),
    backgroundPosition: positions.join(", "),
    backgroundRepeat: "no-repeat",
  };
});

// Helper function to safely get nested properties
function getNested(obj, pathArray) {
  return pathArray.reduce(
    (acc, key) => (acc && acc[key] != null ? acc[key] : undefined),
    obj
  );
}

const priorityConfig = computed(() => {
  const cfg = getNested(props.task, ["mission_config"]) ?? getNested(props.task, ["startConfig"]) ?? getNested(props.task, ["config"]);
  try { return typeof cfg === 'string' ? JSON.parse(cfg) : cfg; } catch { return null; }
});

const getStatusType = priorityStatusType;
</script>

<style scoped>
.source-group {
  overflow-wrap: anywhere;
}
.runtime-details {
  margin-top: 4px;
  color: #666;
}
.runtime-details > summary {
  cursor: pointer;
}
.task-card {
  max-width: 300px;
  min-height: 180px;
  border-radius: 12px;
  box-shadow: 0 1px 6px rgba(0, 0, 0, 0.05);
  transition: all 0.2s ease;
}
.task-card:hover {
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.08);
  transform: translateY(-2px);
}
.task-card.priority-task {
  border: 2px solid #ff9800; /* 橙色边框 */
  box-shadow: 0 4px 12px rgba(255, 152, 0, 0.3); /* 橙色阴影 */
  background-color: #fffaf2; /* 浅橙背景 */
}
.card-header {
  position: relative;
  width: 100%;
  min-height: 40px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 12px;
  box-sizing: border-box;
  z-index: 1;
}
.progress-background {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  width: 100%;
  background-color: #f0f0f0;
  z-index: 0;
  border-radius: 10px 10px 0 0;
  pointer-events: none;
}
.header-content {
  position: relative;
  z-index: 1;
  display: flex;
  justify-content: space-between;
  width: 100%;
  padding: 0 12px;
  font-weight: bold;
  font-size: 14px;
  gap: 6px;
  align-items: center;
}
.runtime-badge {
  max-width: 72%;
  height: auto;
  min-height: 20px;
  padding-top: 2px;
  padding-bottom: 2px;
  flex-shrink: 0;
}
.runtime-badge :deep(.n-tag__content) {
  white-space: normal;
  overflow-wrap: anywhere;
  line-height: 1.35;
}
.task-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
