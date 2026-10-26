<template>
  <n-card
    embedded
    size="small"
    :segmented="true"
    :class="['task-card', task.taskType === 'super' ? 'super-task' : '']"
    style="margin: auto"
    :header-style="{
      padding: '0',
      minHeight: 'auto',
      position: 'relative',
      width: '100%',
    }"
  >
    <template #header>
      <div class="card-header">
        <div class="progress-background" :style="progressBarStyle" />
        <div class="header-content">
          <span class="task-name">{{ task.name }}</span>
          <n-tag class="runtime-badge" :type="getStatusType(displayStatus)" :title="dispatchDetails || undefined" size="small" round>
            {{ displayStatus }}
          </n-tag>
        </div>
      </div>
    </template>
    <details v-if="diagnosticDetails || dispatchDetails" class="operation-diagnostics" style="font-size: 12px; margin-bottom: 4px">
      <summary style="cursor: pointer">运行 / 停止详情</summary>
      <div v-if="diagnosticDetails" style="white-space: pre-wrap; overflow-wrap: anywhere">{{ diagnosticDetails }}</div>
      <div v-if="dispatchDetails" style="overflow-wrap: anywhere"><n-text depth="3">{{ dispatchDetails }}</n-text></div>
    </details>
    <div v-if="startBlockReason" style="font-size: 12px; margin-bottom: 4px">
      <n-text type="warning">{{ startBlockReason }}</n-text>
    </div>
    <div v-if="progress">
      <div style="font-size: 12px; margin-bottom: 4px">
        <n-text type="success" title="成功 (普通+插队)">
          ✓ {{ (progress?.success ?? 0) - (progress?.priority_success ?? 0) }}+{{ progress?.priority_success ?? 0 }}
        </n-text>
        &nbsp;|&nbsp;
        <n-text type="error" title="失败 (普通+插队)">
          ✗ {{ (progress?.error ?? 0) - (progress?.priority_error ?? 0) }}+{{ progress?.priority_error ?? 0 }}
        </n-text>
        &nbsp;|&nbsp;
        <n-text depth="3" title="总计 (普通+插队)">
          Σ {{ (progress?.total ?? 0) - (progress?.priority_total ?? 0) }}+{{ progress?.priority_total ?? 0 }}
        </n-text>
      </div>
    </div>
    <div v-if="progress?.stale" style="font-size: 12px; margin-bottom: 4px">
      <n-text type="warning" :title="progress.lastError || '等待进度统计'">
        {{ progress.lastError ? '进度刷新失败：' + progress.lastError : '进度加载中…' }}
      </n-text>
    </div>
    <div v-if="task.status === 'running'">
      <template v-if="hasConcurrencyConfig">
        <div style="margin-top: 4px; font-size: 12px; color: #555">
          最大并发: {{ maxConcurrent }} &nbsp;|&nbsp; 子任务上限:
          {{ maxMultiSimulationChildren }}
        </div>
      </template>
    </div>
    <div style="font-size: 12px; margin: 4px 0"><n-text :type="taskMutationIssue(task) ? 'warning' : 'default'">{{ taskControlLabel(task) }}</n-text></div>
    <n-space align="center" style="margin-top: 2px">
      <n-switch
        :value="task.isRemote"
        :disabled="!!taskMutationIssue(task)"
        @update:value="(val) => $emit('update:isRemote', val)"
        :checked-value="true"
        :unchecked-value="false"
      >
        <template #checked>远程</template>
        <template #unchecked>本地</template>
      </n-switch>
      <n-tag v-if="task.auth_profile" size="small" :bordered="false" type="info" style="background-color: #e0f2f1; color: #00796b;">
        {{ task.auth_profile }}
      </n-tag>
      <n-tag v-else size="small" :bordered="false" type="info" style="background-color: #e0f2f1; color: #00796b;">
        user1
      </n-tag>
    </n-space>
      <n-space wrap size="small" style="margin-top: 10px">
        <n-button size="small" @click="$emit('view')">
          <template #icon>[ ]</template>查看
        </n-button>
        <n-button
          size="small"
          @click="$emit('generateList')"
          :disabled="!!taskMutationIssue(task) || !canGenerate(task, generationBusy)"
          :loading="isGenerating(task, generationBusy)"
        >
          <template #icon>=</template>{{ isGenerating(task, generationBusy) ? '生成中…' : task.status === 'generation_unknown' ? '生成状态待核验' : '生成list' }}
        </n-button>
        <n-button
          size="small"
          @click="$emit('syncRemote')"
          :disabled="!!taskMutationIssue(task) || task.status === 'running' || generationBusyOrUnknown(task, generationBusy)"
        >
          <template #icon>-></template>发送到远程
        </n-button>
        <n-button size="small" :disabled="!!startBlockReason" :title="startBlockReason || undefined" @click="$emit('start')">
          <template #icon>>></template>启动
        </n-button>
        <n-button size="small" :disabled="taskRuntimeStatus?.state === 'checking' || operationBusy || !!taskMutationIssue(task)" @click="$emit('checkRuntime')">运行诊断（只读，可选）</n-button>
        <n-button size="small" :disabled="!!taskMutationIssue(task) || generationBusyOrUnknown(task, generationBusy)" @click="$emit('pause')">
          <template #icon>| |</template>暂停
        </n-button>
        <n-button size="small" type="error" :disabled="!!taskMutationIssue(task) || generationBusyOrUnknown(task, generationBusy)" @click="$emit('delete')">
          <template #icon>x</template>删除
        </n-button>
      </n-space>
    </n-card>
  </template>

<script>
// Public exports retained for callers/tests; presentation shared with Priority.
export { RUNTIME_MAX_AGE as REGULAR_RUNTIME_MAX_AGE, DISPATCH_MAX_AGE as REGULAR_DISPATCH_MAX_AGE,
  regularDispatchState, regularDisplayStatus } from '../../utils/taskDiagnostics.js';
</script>

<script setup>
import { computed, ref, watch, onMounted, onUnmounted } from 'vue';
import { taskMutationIssue, taskControlLabel, RUNTIME_MAX_AGE, regularDispatchState, regularDisplayStatus, taskDiagnosticDetails, nextRuntimeExpiry, taskStartBlockReason } from '../../utils/taskDiagnostics.js';
import { taskStopDisplayStatus, taskStopProofIssue } from '../../stores/taskStore.js';
import { isGenerating, canGenerate, generationBusyOrUnknown } from '../../utils/taskGeneration.js';

const props = defineProps({
  generationBusy: { type: Boolean, default: false },
  operationBusy: { type: Boolean, default: false },
  task: {
    type: Object,
    required: true,
  },
  taskRuntimeStatus: {
    type: Object,
    default: null,
  },
  stopOperation: {
    type: Object,
    default: null,
  },
  progress: {
    type: Object,
    default: () => ({}),
  },
});

defineEmits(['update:isRemote', 'view', 'generateList', 'syncRemote', 'start', 'pause', 'delete', 'checkRuntime']);

// A one-shot local expiry (not another native poll) invalidates Vue's cached
// computed badge even when no props change after a positive observation.
const runtimeNow = ref(Date.now());
let mounted = false;
let expiryTimer;
const refreshRuntimeExpiry = () => {
  clearTimeout(expiryTimer);
  runtimeNow.value = Date.now();
  const observation = props.taskRuntimeStatus;
  if (mounted && observation) {
    const expiresAt = nextRuntimeExpiry(props.task, observation, runtimeNow.value);
    const remaining = expiresAt - runtimeNow.value;
    if (remaining > 0 && remaining <= RUNTIME_MAX_AGE) expiryTimer = setTimeout(refreshRuntimeExpiry, remaining);
  }
};
const stopExpiryWatch = watch(() => props.taskRuntimeStatus, refreshRuntimeExpiry, { deep: true });
onMounted(() => { mounted = true; refreshRuntimeExpiry(); });
onUnmounted(() => { mounted = false; clearTimeout(expiryTimer); stopExpiryWatch(); });
const stopStatus = computed(() => taskStopDisplayStatus(props.task, props.stopOperation));
const startBlockReason = computed(() => taskStartBlockReason(props.task, {
  busy: props.operationBusy || props.generationBusy, stopping: stopStatus.value === 'stopping',
}));
const displayStatus = computed(() => regularDisplayStatus(props.task, props.taskRuntimeStatus, stopStatus.value, runtimeNow.value));
const diagnosticDetails = computed(() => taskDiagnosticDetails(props.task, props.taskRuntimeStatus, props.stopOperation, stopStatus.value, runtimeNow.value, taskStopProofIssue(props.task)));
const dispatchDetails = computed(() => {
  if (!['请求结果待核验（派发暂停，消费者存活）', '等待配额恢复（派发暂停，消费者存活）', '消费者存活（可派发）'].includes(displayStatus.value)) return '';
  const d = regularDispatchState(props.task, props.taskRuntimeStatus, runtimeNow.value);
  if (!d) return '';
  const q = d.quota;
  const quota = q?.remaining != null && q?.limit != null
    ? `${q.budgetConfirmed ? '配额快照' : '上次配额（未核验）'}: ${q.remaining}/${q.limit}` : '配额未核验';
  const reset = Number.isSafeInteger(d.resetAt) ? `重置时间（UTC）: ${new Date(d.resetAt).toISOString()}` : '快照未提供重置时间';
  return [d.submissionReason, quota, reset].filter(Boolean).join(' · ');
});
const getStatusType = (status) =>
  ["done", "paused"].includes(status) ? "success" : /失败|超时|不匹配|无效|不符合/.test(status) ? "error"
    : props.task.status === 'running' || props.stopOperation ? "warning" : "default";

const progressBarStyle = computed(() => {
  const p = props.progress || {};
  const success = typeof p.success === "number" ? p.success : 0;
  const error = typeof p.error === "number" ? p.error : 0;
  const total = typeof p.total === "number" && p.total > 0 ? p.total : 1;
  const priority_success = typeof p.priority_success === "number" ? p.priority_success : 0;
  const priority_error = typeof p.priority_error === "number" ? p.priority_error : 0;

  const nonPrioritySuccess = Math.max(success - priority_success, 0);
  const nonPriorityError = Math.max(error - priority_error, 0);
  
  const greenPercent = (nonPrioritySuccess / total) * 100;
  const redPercent = (nonPriorityError / total) * 100;
  const pGreenPercent = (priority_success / total) * 100;
  const pRedPercent = (priority_error / total) * 100;

  const isSuper = props.task.taskType === "super";
  const mainColor = isSuper ? "#64b5f6" : "#a1e3a1";
  const errorColor = "#ff5252";
  const pMainColor = "#caa969";
  const pErrorColor = "#ffb74d";
  const bgColor = "#f0f0f0";

  const layers = [];
  const sizes = [];
  const positions = [];

  // Layer 1: Normal Success
  layers.push(`linear-gradient(to right, ${mainColor}, ${mainColor})`);
  sizes.push(`${greenPercent}% 100%`);
  positions.push(`left top`);

  // Layer 2: Normal Error
  layers.push(`linear-gradient(to right, ${errorColor}, ${errorColor})`);
  sizes.push(`${redPercent}% 100%`);
  positions.push(`${greenPercent}% 0`);

  // Layer 3: Priority Success
  layers.push(`linear-gradient(to right, ${pMainColor}, ${pMainColor})`);
  sizes.push(`${pGreenPercent}% 100%`);
  positions.push(`${greenPercent + redPercent}% 0`);

  // Layer 4: Priority Error
  layers.push(`linear-gradient(to right, ${pErrorColor}, ${pErrorColor})`);
  sizes.push(`${pRedPercent}% 100%`);
  positions.push(`${greenPercent + redPercent + pGreenPercent}% 0`);

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

function getNested(obj, pathArray) {
  return pathArray.reduce(
    (acc, key) => (acc && acc[key] != null ? acc[key] : undefined),
    obj
  );
}

const maxConcurrent = computed(() => {
  return (
    getNested(props.task, ["mission_config", "max_concurrent"]) ??
    getNested(props.task, ["config", "max_concurrent"]) ??
    "-"
  );
});

const maxMultiSimulationChildren = computed(() => {
  return (
    getNested(props.task, ["mission_config", "max_multi_simulation_children"]) ??
    getNested(props.task, ["config", "max_multi_simulation_children"]) ??
    "-"
  );
});

const hasConcurrencyConfig = computed(() => {
  const mc = maxConcurrent.value;
  const mm = maxMultiSimulationChildren.value;
  return props.task.status === "running" && (mc !== "-" || mm !== "-");
});
</script>

<style scoped>
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
.task-card.super-task {
  border: 2px solid #2196f3; /* 蓝色边框 */
  box-shadow: 0 4px 12px rgba(33, 150, 243, 0.3); /* 蓝色阴影 */
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
  background: linear-gradient(to right, #18a058, #f0f0f0);
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
