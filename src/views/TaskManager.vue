<template>
  <div style="padding: 0px">
    <TaskManagerToolbar
      @add-task="showAddModal = true"
      @add-super-task="showAddSuperModal = true"
      @add-priority-task="showAddPriorityModal = true"
    />

    <n-empty v-if="taskIsEmpty" description="暂无任务" />

    <n-collapse default-expanded-names="others">
      <!-- Priority 任务 -->
      <n-collapse-item title="Priority 任务" name="priority">
        <n-empty v-if="priorityTasks.length === 0" description="暂无 Priority 任务" />
        <n-grid
          x-gap="20"
          y-gap="20"
          responsive="screen"
          style="
            display: grid;
            grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
          "
        >
          <n-gi
            v-for="task in priorityTasks"
            :key="typeof task._id === 'object' ? task._id.$oid : task._id"
            style="margin-bottom: 20px"
          >
            <n-text v-if="taskOperationDetail(task)" type="warning">{{ taskOperationDetail(task) }}</n-text>
            <PriorityTaskCard
              :task="task"
              :progress="taskProgressMap[task.name]"
              :task-runtime-status="taskRuntimeStatus[task.name]"
              :stop-operation="taskStopOperationMap[task.name]"
              @update:is-remote="(val) => updateIsRemote(task._id, val)"
              @start="openStartModal(task)"
              @update="openUpdateModal(task)"
              @view="viewTask(task._id)"
              @pause="pauseTask(task)"
              @delete="deleteTask(task._id)"
            />
          </n-gi>
        </n-grid>
      </n-collapse-item>

      <!-- 普通 / Super 任务 -->
      <n-collapse-item title="普通 / Super 任务" name="others">
        <n-empty v-if="otherTasks.length === 0" description="暂无任务" />
        <n-grid
          x-gap="20"
          y-gap="20"
          responsive="screen"
          style="
            display: grid;
            grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
          "
        >
          <n-gi
            v-for="task in otherTasks"
            :key="typeof task._id === 'object' ? task._id.$oid : task._id"
            style="margin-bottom: 20px"
          >
            <n-text v-if="taskOperationDetail(task)" type="warning">{{ taskOperationDetail(task) }}</n-text>
            <RegularTaskCard
              :task="task"
              :generation-busy="Boolean(generationRequests[getObjectId(task._id)])"
              :operation-busy="taskOperationBusy(task)"
              @check-runtime="checkTaskStatus(task)"
              :progress="taskProgressMap[task.name]"
              :task-runtime-status="taskRuntimeStatus[task.name]"
              :stop-operation="taskStopOperationMap[task.name]"
              @update:is-remote="(val) => updateIsRemote(task._id, val)"
              @view="viewTask(task._id)"
              @generate-list="generateList(task._id)"
              @sync-remote="syncRemoteTask(task.name)"
              @start="openStartModal(task)"
              @pause="pauseTask(task)"
              @delete="deleteTask(task._id)"
            />
          </n-gi>
        </n-grid>
      </n-collapse-item>
    </n-collapse>

    <n-modal :show="!!detailTaskId" preset="card" title="任务详情" style="max-width: 720px" @update:show="value => { if (!value) closeTaskDetails(); }">
      <TaskDetailsPanel :task-id="detailTaskId" :task="detailTask"
        :progress="detailTask ? taskProgressMap[detailTask.name] : undefined"
        :runtime="detailTask ? taskRuntimeStatus[detailTask.name] : undefined"
        :stop-operation="detailTask ? taskStopOperationMap[detailTask.name] : undefined"
        @browse-alpha-library="router.push({ name: 'Data' })" />
    </n-modal>

    <!-- Modals -->
    <AddTaskModal v-model:show="showAddModal" @add-task="handleAddTask" />
    <AddSuperTaskModal
      v-model:show="showAddSuperModal"
      @add-super-task="handleAddSuperTask"
    />
    <AddPriorityTaskModal
      v-model:show="showAddPriorityModal"
      @add-priority-task="handleAddPriorityTask"
    />
    <StartTaskModal
      v-model:show="showStartModal"
      :task="selectedTask"
      :isSuper="getIsSuper(selectedTask)"
      :isPriority="getIsPriority(selectedTask)"
      @start-task="handleStartTask"
      @start-super-task="handleStartSuperTask"
      @start-priority-task="handlePriorityStartTask"
    />
    <UpdateTaskModal
      v-model:show="showUpdateModal"
      :task="selectedTaskForUpdate"
      :isSuper="isSuperForUpdate"
      :isPriority="isPriorityForUpdate"
      @update-task="handleUpdateTask"
      @update-priority-task="handleUpdatePriorityTask"
    />
  </div>
</template>

<script>
import { TASK_STAGE_MARKER, validRuntimeStage, errorStage, BUSINESS_DIAGNOSTIC_MARKER, validBusinessDiagnostic } from '../utils/taskDiagnostics.js';
// Wire metadata is constructed by Native, never by the process script. Reject
// malformed/duplicated/non-terminal markers without weakening process parsing.
export const DISPATCH_MAX_AGE = 120000;
const DISPATCH_MARKER = 'QUANTNIGHT_DISPATCH_STATE ';
export function validDispatchState(value, target, now) {
  const text = (v, max) => typeof v === 'string' && v.length > 0 && v.length <= max && !/[\x00-\x1f\x7f]/.test(v);
  const counter = v => v === null || (Number.isSafeInteger(v) && v >= 0);
  const q = value?.quota;
  return value?.schemaVersion === 1 && value.taskId === target.taskId && value.name === target.name &&
    typeof value.isRemote === 'boolean' && value.isRemote === target.isRemote &&
    value.authProfile === target.authProfile && /^[A-Za-z0-9_-]{1,100}$/.test(value.authProfile) &&
    ['ready', 'waiting_reconciliation', 'waiting_quota_reset'].includes(value.submissionState) &&
    (value.submissionReason === null ? value.submissionState === 'ready' : text(value.submissionReason, 1024)) &&
    Number.isSafeInteger(value.heartbeatAt) && value.heartbeatAt > 0 && value.heartbeatAt <= now + 5000 &&
    now - value.heartbeatAt < DISPATCH_MAX_AGE && /^[a-fA-F0-9]{32}$/.test(value.consumerRunId) &&
    Number.isSafeInteger(value.pid) && value.pid > 1 && value.pid <= 4294967295 &&
    !!q && counter(q.limit) && q.limit !== 0 && counter(q.remaining) && counter(q.stopFloor) &&
    (q.remaining === null || q.limit === null || q.remaining <= q.limit) &&
    (q.stopFloor === null || q.limit === null || q.stopFloor <= q.limit) && typeof q.budgetConfirmed === 'boolean' &&
    (value.submissionState !== 'waiting_quota_reset' || [q.limit, q.remaining, q.stopFloor].every(v => v !== null)) &&
    (value.resetAt === null || (Number.isSafeInteger(value.resetAt) && value.resetAt > 0 && value.resetAt <= 8640000000000000));
}

// Native stdout is a process observation, not control state or platform progress.
export function normalizeTaskRuntime(target, output, observedAt, error = null) {
  const input = typeof output === 'string' ? output.trim() : '';
  const allLines = input.split(/\r?\n/).filter(Boolean);
  let stage = null;
  let detail = null;
  let stageError = null;
  // Strip every marker-bearing line, but malformed/foreign diagnostics poison
  // the process observation. Marker text is NEVER parsed as a Pane or receipt.
  const processLines = allLines.filter(line => {
    if (!line.includes('QUANTNIGHT_TASK_STAGE')) return true;
    try {
      if (!line.startsWith(TASK_STAGE_MARKER)) throw new Error('阶段 marker 位置无效');
      const value = JSON.parse(line.slice(TASK_STAGE_MARKER.length));
      if (!validRuntimeStage(value, target)) throw new Error('阶段记录校验失败：任务身份、运行主机、操作类型或字段格式有差异');
      stage = value.stage; detail = value.message;
    } catch (err) { stageError = `阶段诊断校验失败：${String(err)}`; }
    return false;
  });
  const businessMarkers = processLines.filter(line => line.includes('QUANTNIGHT_BUSINESS_DIAGNOSTIC'));
  let businessDiagnostic = null;
  if (!error && !stageError && businessMarkers.length === 1 && processLines.at(-1) === businessMarkers[0] && businessMarkers[0].startsWith(BUSINESS_DIAGNOSTIC_MARKER)) {
    try {
      const value = JSON.parse(businessMarkers[0].slice(BUSINESS_DIAGNOSTIC_MARKER.length));
      if (validBusinessDiagnostic(value, target, observedAt)) businessDiagnostic = value;
    } catch { /* Not process/control authority or an exit cause. */ }
  }
  const evidenceLines = processLines.filter(line => !line.includes('QUANTNIGHT_BUSINESS_DIAGNOSTIC'));
  const markers = evidenceLines.filter(line => line.includes('QUANTNIGHT_DISPATCH_STATE'));
  const lines = evidenceLines.filter(line => !line.includes('QUANTNIGHT_DISPATCH_STATE'));
  let dispatch = null;
  if (!error && !stageError && markers.length === 1 && evidenceLines.at(-1) === markers[0] && markers[0].startsWith(DISPATCH_MARKER)) {
    try {
      const value = JSON.parse(markers[0].slice(DISPATCH_MARKER.length));
      if (validDispatchState(value, target, observedAt)) dispatch = value;
    } catch { /* Missing/invalid capability is NOT a quota wait. */ }
  }
  const raw = lines.join('\n');
  const positive = /^✅ Pane \d+ \(pid=[1-9]\d*\) is running: (?:python(?:\d+(?:\.\d+)*)?|uv)$/;
  const negative = /^❌ Pane \d+ \(pid=[1-9]\d*\) (?:is running: (?:bash|sh|zsh|sleep)|exited \(retained tmux pane\))$/;
  const recognized = !error && !stageError && lines.length > 0 && lines.every(line => positive.test(line) || negative.test(line));
  const absenceLine = `❌ Session '${target.name}' 不存在 (${target.isRemote ? 'remote' : 'local'})`;
  // Absence is a normal read-only paused observation, never a DB write or proof
  // inferred from a progress event. Reject mixed/mismatched/error output.
  const absent = !error && !stageError && lines.length === 1 && lines[0] === absenceLine;
  const state = absent ? 'session_absent' : recognized ? (lines.some(line => positive.test(line)) ? 'running' : 'not_running') : 'unknown';
  return { ...target, observedAt, state, raw, dispatch, businessDiagnostic,
    stage: errorStage(error) ?? stage, detail,
    diagnostic: stageError || (lines.length ? '进程输出含未识别行或无效 PID/进程格式。' : '原生检查返回空进程输出。'),
    error: error ? String(error) : null };
}
</script>

<script setup>
import { ref, onMounted, onUnmounted, watch, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useRoute, useRouter } from "vue-router";
import { useTaskStore, validStopReceipt, taskStopDisplayStatus } from "../stores/taskStore";
import { createTaskFetcher, createTaskProgressLifecycle, mutateTaskSource } from "../utils/taskProgress.js";
import { newTaskRequestId, validOperationStage, taskRuntimeTarget, sameRuntimeIdentity, taskStartBlockReason, isCompletedTask, taskMutationIssue, taskStartOutcome, taskStartFailure, taskStartJoinEvidence, taskMatchesJoinEvidence, taskLaunchValue, taskLaunchMatches } from '../utils/taskDiagnostics.js';
import { canGenerate } from '../utils/taskGeneration.js';
import { storeToRefs } from "pinia";
import { useMessage, useDialog } from "naive-ui";

// Import new components
import TaskManagerToolbar from "../components/TaskCards/TaskManagerToolbar.vue";
import PriorityTaskCard from "../components/TaskCards/PriorityTaskCard.vue";
import RegularTaskCard from "../components/TaskCards/RegularTaskCard.vue";
import TaskDetailsPanel from "../components/TaskCards/TaskDetailsPanel.vue";

// Import moved modals
import AddTaskModal from "../components/modals/AddTaskModal.vue";
import AddSuperTaskModal from "../components/modals/AddSuperTaskModal.vue";
import AddPriorityTaskModal from "../components/modals/AddPriorityTaskModal.vue";
import StartTaskModal from "../components/modals/StartTaskModal.vue";
import UpdateTaskModal from "../components/modals/UpdateTaskModal.vue";

// Store and Router
const taskStore = useTaskStore();
const { tasks, taskProgressMap, taskStopOperationMap } = storeToRefs(taskStore);
const router = useRouter();
const route = useRoute();

// UI Hooks
const message = useMessage();
const dialog = useDialog();

// Modal visibility state
const showAddModal = ref(false);
const showAddSuperModal = ref(false);
const showAddPriorityModal = ref(false);
const showStartModal = ref(false);
const showUpdateModal = ref(false);

// State for modals
const selectedTask = ref(null);
const selectedTaskForUpdate = ref(null);
const isSuperForUpdate = ref(false);
const isPriorityForUpdate = ref(false);

// Other reactive state
const taskRuntimeStatus = ref({});
const runtimeRequests = new Map();
const runtimeTarget = (task) => ({ ...taskRuntimeTarget(task), controlStatus: task.status });
const matchesRuntimeTarget = (task, target) => sameRuntimeIdentity(task, target) &&
  (task.status === target.controlStatus || isCompletedTask(task));
const invalidateRuntime = (name) => {
  delete taskRuntimeStatus.value[name];
  runtimeRequests.get(name)?.cancelTimer?.();
  runtimeRequests.delete(name);
};
const pendingStops = new Map();
let disposed = false;

// Computed properties for task lists
const taskIsEmpty = computed(() => tasks.value.length === 0);
const priorityTasks = computed(() => (tasks.value || []).filter((t) => t.taskType === "priority"));
const otherTasks = computed(() => (tasks.value || []).filter((t) => t.taskType !== "priority"));

// --- Data Fetching and Store Management ---
const fetchTasks = createTaskFetcher({
  load: () => invoke("get_all_tasks"),
  replace: (activeTasks) => taskStore.replaceTasks(activeTasks),
  refresh: () => invoke("refresh_task_progress"),
  isActive: () => !disposed,
  onError: (error) => {
    console.error("加载任务失败", error);
    message.error("加载任务失败");
  },
  onRefreshError: (error) => console.error("刷新任务进度失败", error),
});

// --- Backend Event Handling ---
const handleTaskProgressUpdate = (progress) => {
  if (!disposed) taskStore.updateProgress(progress);
};
const handleTaskProgressError = (error) => {
  if (!disposed) taskStore.updateProgressError(error);
};
const handleTaskProgressRefreshError = (error) => {
  if (!disposed) taskStore.markProgressStale(error);
};

// Live stages are diagnostics only, strictly scoped to a still-pending request.
const handleTaskOperationStage = (event) => {
  if (disposed || !validOperationStage(event)) return false;
  if (event.operation === 'runtime') {
    const request = runtimeRequests.get(event.name);
    if (!request || request.completed || event.requestId !== request.requestId ||
        event.taskId !== request.target.taskId || event.isRemote !== request.target.isRemote ||
        !matchesRuntimeTarget(tasks.value.find(task => task.name === event.name), request.target)) return false;
    taskRuntimeStatus.value[event.name] = { ...taskRuntimeStatus.value[event.name], stage: event.stage, detail: event.message };
    return true;
  }
  const entry = [...pendingStops.values()].find(({ operation }) => operation.requestId === event.requestId);
  if (!entry || entry.completed) return false;
  return taskStore.updateTaskStopStage(entry.target, entry.operation, event);
};

// --- Task Actions (invoking Rust backend) ---
const getObjectId = (id) => (id && typeof id === "object" && "$oid" in id ? id.$oid : id);
const allowTaskMutation = (task) => {
  const issue = taskMutationIssue(task);
  if (issue) { message.warning(issue); return false; }
  return true; // Unknown/old DTO still relies on native exact local control ID.
};

const generationRequests = ref({});
const generateList = async (id) => {
  const key = getObjectId(id);
  const task = tasks.value.find(item => getObjectId(item._id) === key);
  if (!allowTaskMutation(task) || !canGenerate(task, Boolean(generationRequests.value[key])) || taskOperationBusy(task)) return;
  generationRequests.value[key] = true; // disable immediately, before awaiting native
  try {
    await oncePerTask(key, () => invoke("generate_list", { id: key }), { kind: 'generate' });
    if (!disposed) message.success("列表生成完成");
  } catch (err) {
    console.error("生成列表失败", err);
    if (!disposed) message.error("生成列表失败: " + err);
  } finally {
    if (!disposed) {
      await fetchTasks();
      delete generationRequests.value[key];
    }
  }
};

// A target checkbox is not migration authority. Native rejects unsupported
// intent changes; only shared core sync may change the actual execution host.
const setRemoteSource = (task, isRemote) => mutateTaskSource({
  fetch: fetchTasks,
  update: () => invoke("update_remote_status", {
    id: getObjectId(task._id), updates: { is_remote: isRemote },
  }),
});

// One operation owner per task, including the native start -> requested pause handoff.
const mutationRequests = ref(new Map());
const monotonicNow = () => performance.now();
const pauseJoinBudget = Object.freeze({ ui: 120000, fresh: 8000, adapter: 8000, core: 75000, reply: 2000 });
const pauseDispatchReserve = pauseJoinBudget.adapter + pauseJoinBudget.core + pauseJoinBudget.reply;
const deferredOperation = () => {
  let resolve, reject;
  const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
};
const startModalTarget = ref(null);
const matchesStartModalTarget = (task, target) => !!task && !!target && task.status !== 'deactive' &&
  getObjectId(task._id) === target.taskId && task.name === target.name &&
  (task.isRemote === true) === target.isRemote && (task.auth_profile ?? 'user1') === target.authProfile &&
  (task.taskType ?? 'regular') === target.taskType && (task.taskType !== 'priority' ||
    (task.targetRemote ?? null) === target.targetRemote);
const taskOperationBusy = (task) => mutationRequests.value.has(getObjectId(task._id)) ||
  [...pendingStops.values()].some(entry => entry.target._id === getObjectId(task._id));
const taskOperationDetail = (task) => {
  const intent = mutationRequests.value.get(getObjectId(task._id))?.pauseIntent;
  if (!intent || intent.completed) return '';
  return intent.stopSent ? '暂停请求已发送，等待原停止核验与回执'
    : intent.phase === 'fresh' ? '已请求暂停：正在核验本次启动身份，尚未发送停止'
      : '已请求在本次启动结束后暂停：等待原启动回执，尚未发送停止';
};
const startBlockReason = (task) => taskStartBlockReason(task, {
  busy: taskOperationBusy(task) || Boolean(generationRequests.value[getObjectId(task?._id)]),
  stopping: taskStopDisplayStatus(task, taskStopOperationMap.value[task?.name]) === 'stopping',
});

const releaseMutation = (id, entry) => {
  if (mutationRequests.value.get(id) === entry && entry.actionFinished &&
      (!entry.pauseIntent || entry.pauseIntent.completed)) mutationRequests.value.delete(id);
};
const oncePerTask = async (id, action, { joinStop = false, kind = 'mutation', binding = null } = {}) => {
  if (disposed || mutationRequests.value.has(id) || (!joinStop &&
      [...pendingStops.values()].some(entry => entry.target._id === id))) return false;
  mutationRequests.value.set(id, { kind, binding, operationId: newTaskRequestId(kind),
    nativeSent: false, nativeOutcomePromise: null, actionDone: deferredOperation(), actionFinished: false,
    pauseIntent: null, invalidated: false, seenLaunches: new Set(binding ? [binding.launchAttemptId] : []),
    seenHosts: new Set(binding ? [binding.actualRemote] : []) });
  const entry = mutationRequests.value.get(id);
  try { return await action(entry); }
  finally { entry.actionFinished = true; entry.actionDone.resolve(); releaseMutation(id, entry); }
};

const syncRemoteTask = async (taskName) => {
  try {
    const task = tasks.value.find((task) => task.name === taskName);
    if (!task) throw new Error("任务列表缺少所选任务记录");
    if (!allowTaskMutation(task)) return;
    const target = { taskId: getObjectId(task._id), alphaMissionList: task.name, expectedRemote: task.isRemote === true };
    await oncePerTask(target.taskId, async () => {
      await mutateTaskSource({ fetch: fetchTasks,
        update: () => invoke("sync_remote_task", target),
      });
      if (!disposed) message.success("任务安全同步完成；实际执行端由共享核心确认");
      await fetchTasks();
    });
  } catch (err) {
    console.error("同步失败", err);
    if (!disposed) message.error("任务同步失败: " + err);
  }
};

const updateIsRemote = async (id, isRemote) => {
  try {
    const targetTask = tasks.value.find((task) => getObjectId(task._id) === getObjectId(id));
    if (!targetTask) throw new Error("任务列表缺少所选任务记录");
    if (!allowTaskMutation(targetTask)) return;
    await oncePerTask(getObjectId(id), async () => {
      await setRemoteSource(targetTask, isRemote);
      if (!disposed) message.success(`运行方式已更新为 ${isRemote ? "远程" : "本地"}`);
      await fetchTasks();
    });
  } catch (err) {
    console.error("更新 isRemote 失败", err);
    if (!disposed) message.error("运行方式更新失败: " + err);
  }
};

const performPauseTask = async (task, { reportError = true, refreshAfter = true, joinDeadline = null,
  expectedLaunchAttemptId, expectedLaunchAbsent, onDispatch } = {}) => {
  // Capture before awaiting: the current task/host/launch may be replaced meanwhile.
  const target = { _id: getObjectId(task._id), name: task.name, isRemote: task.isRemote === true,
    ...(expectedLaunchAttemptId != null ? { expectedLaunchAttemptId } : {}),
    ...(expectedLaunchAbsent === true ? { expectedLaunchAbsent: true } : {}),
    ...(joinDeadline !== null ? { expectedTaskType: task.taskType ?? 'regular', taskType: task.taskType ?? 'regular',
      expectedAuthProfile: task.auth_profile ?? 'user1', auth_profile: task.auth_profile ?? 'user1' } : {}),
    ...((expectedLaunchAttemptId != null || expectedLaunchAbsent === true) && Object.hasOwn(task, 'launchAttemptId')
      ? { launchAttemptId: task.launchAttemptId } : {}) };
  const remainingMs = joinDeadline === null ? null : Math.floor(joinDeadline - monotonicNow());
  if (remainingMs !== null && (!Number.isSafeInteger(remainingMs) || remainingMs <= 0 || remainingMs > pauseJoinBudget.ui)) return false;
  if (disposed) return false;
  const operation = taskStore.beginTaskStop(target);
  if (!operation) return false; // missing/replaced task or an already pending stop
  const pending = { target, operation, completed: false, failureSource: 'native' };
  pendingStops.set(operation.generation, pending);
  let receipt;
  let timer;
  let committed = false;
  try {
    // Reuse both invalidation barriers. A resolved void command or stdout text
    // is not proof: native must verify process exit AND commit control state.
    await mutateTaskSource({
      fetch: fetchTasks,
      update: async () => {
        try {
          receipt = await Promise.race([
            (() => {
              onDispatch?.();
              return invoke("pause_task", { taskName: target.name, taskId: target._id, isRemote: target.isRemote, requestId: operation.requestId,
                ...(expectedLaunchAttemptId != null ? { expectedLaunchAttemptId } : {}),
                ...(expectedLaunchAbsent === true ? { expectedLaunchAbsent: true } : {}),
                ...(remainingMs !== null ? { remainingMs } : {}) });
            })(),
            new Promise((_, reject) => {
              timer = setTimeout(() => {
                pending.completed = true;
                pending.failureSource = 'client_timeout';
                reject(new Error("停止确认超时（120 秒）；最终暂停回执待核对"));
              }, remainingMs ?? 120000);
              pending.cancelTimer = () => clearTimeout(timer);
            }),
          ]);
        } finally { pending.completed = true; clearTimeout(timer); }
        if (!validStopReceipt(target, receipt)) {
          pending.failureSource = 'receipt_validation';
          throw new Error("暂停回执校验失败：回执字段、任务身份或执行主机有差异");
        }
      },
      commit: () => { if (!disposed) committed = taskStore.confirmTaskPaused(target, receipt, operation); },
    });
  } catch (error) {
    if (!disposed) {
      taskStore.markTaskStopUnconfirmed(target, operation, error, pending.failureSource);
      if (reportError) message.error("停止操作失败：" + String(error));
    }
    throw error;
  } finally { pendingStops.delete(operation.generation); }
  if (!disposed && refreshAfter) await fetchTasks();
  return joinDeadline === null ? true : committed;
};

// Original pause executor: public pause and an already-owned archive share
// this path and pendingStops promise, never acquire a second mutation lock.
const requestPauseTask = (task, options) => {
  const target = { taskId: getObjectId(task._id), name: task.name, isRemote: task.isRemote === true };
  const promise = performPauseTask(task, options);
  const pending = [...pendingStops.values()].find(entry => entry.target._id === target.taskId &&
    entry.target.name === target.name && entry.target.isRemote === target.isRemote);
  if (pending && !pending.promise) pending.promise = promise;
  return promise;
};

// Cancellation only withdraws an unsent continuation; it never cancels a sent stop.
const cancelPauseIntent = (intent, reason) => {
  if (!intent || intent.completed || intent.cancelled) return;
  intent.cancelled = true; intent.reason = reason;
  if (intent.phase === 'fresh') fetchTasks.invalidate();
  intent.rejectWait?.(new Error(reason));
};
const rememberStartSnapshot = (entry, task) => {
  const b = entry.binding;
  if (!b || entry.kind !== 'start') return;
  if (!task || task.status === 'deactive' || getObjectId(task._id) !== b.taskId || task.name !== b.name ||
      (task.taskType ?? 'regular') !== b.taskType || (task.auth_profile ?? 'user1') !== b.authProfile || taskMutationIssue(task) ||
      (![(b.actualRemote), b.remote].includes(task.isRemote === true)) || taskLaunchValue(task) === undefined) {
    entry.invalidated = true;
  } else {
    entry.seenLaunches.add(taskLaunchValue(task)); entry.seenHosts.add(task.isRemote === true);
    if (entry.seenLaunches.size > 2 || (entry.pauseIntent?.evidence &&
        ([...entry.seenLaunches].some(launch => launch !== b.launchAttemptId && launch !== entry.pauseIntent.evidence.launchAttemptId) ||
         [...entry.seenHosts].some(host => host !== b.actualRemote && host !== entry.pauseIntent.evidence.isRemote)))) entry.invalidated = true;
    if (entry.pauseIntent?.phase === 'stopping' && !taskMatchesJoinEvidence(task, entry.pauseIntent.evidence)) entry.invalidated = true;
  }
  if (entry.invalidated) cancelPauseIntent(entry.pauseIntent, '任务身份/执行端/launch已变化，取消本次后续暂停');
};
const waitForPauseIntent = async (intent, promise, milliseconds, reason) => {
  if (disposed || intent.cancelled || (milliseconds !== null && (!Number.isFinite(milliseconds) || milliseconds <= 0))) {
    // The native/read promise may already have been created synchronously.
    // Observe its late failure only to drain it; cancellation never authorizes a stop.
    Promise.resolve(promise).catch(() => {});
    throw new Error(intent.reason || reason);
  }
  let timer, rejectWait;
  const aborted = new Promise((_, reject) => {
    rejectWait = reject; intent.rejectWait = reject;
    if (milliseconds !== null) timer = setTimeout(() => cancelPauseIntent(intent, reason), milliseconds);
  });
  try { return await Promise.race([promise, aborted]); }
  finally { clearTimeout(timer); if (intent.rejectWait === rejectWait) intent.rejectWait = null; }
};
const joinOwnStartPause = (task, entry) => {
  const b = entry.binding;
  if (!b || getObjectId(task._id) !== b.taskId || task.name !== b.name ||
      (task.taskType ?? 'regular') !== b.taskType || (task.auth_profile ?? 'user1') !== b.authProfile) {
    message.warning('本次暂停与原启动身份不符，未发送停止'); return Promise.resolve(false);
  }
  if (entry.pauseIntent) return entry.pauseIntent.promise;
  const completion = deferredOperation();
  entry.pauseIntent = { phase: 'waiting_start', deadline: monotonicNow() + pauseJoinBudget.ui,
    completed: false, cancelled: false, stopSent: false, reason: '', evidence: null,
    rejectWait: null, promise: completion.promise };
  const intent = entry.pauseIntent; // use the same reactive owner identity everywhere
  rememberStartSnapshot(entry, tasks.value.find(item => getObjectId(item._id) === b.taskId));
  const notice = message.loading('已请求在本次启动结束后暂停；等待启动回执，尚未发送停止…', { duration: 0 });
  const owned = () => !disposed && !entry.invalidated && !intent.cancelled &&
    mutationRequests.value.get(b.taskId) === entry && entry.pauseIntent === intent;
  const run = async () => {
    try {
      const waitUntil = intent.deadline - pauseJoinBudget.fresh - pauseDispatchReserve;
      let raw;
      try {
        raw = await waitForPauseIntent(intent, entry.nativeOutcomePromise,
          waitUntil - monotonicNow(), '等待本次启动结果超时，停止未发送');
      } catch (error) {
        if (!owned()) throw error;
        const partial = taskStartFailure(error, b);
        if (!partial) throw error;
        raw = partial;
      }
      intent.evidence = taskStartJoinEvidence(raw, b);
      rememberStartSnapshot(entry, tasks.value.find(item => getObjectId(item._id) === b.taskId));
      if (!owned()) throw new Error(intent.reason || '原操作已失效');
      // Sequencing only: receipt above, not this UI barrier, supplies authority.
      await waitForPauseIntent(intent, entry.actionDone.promise,
        waitUntil - monotonicNow(), '启动本地收尾超时，停止未发送');
      if (!owned()) throw new Error(intent.reason || '原操作已失效');
      intent.phase = 'fresh';
      const freshAllowance = Math.min(pauseJoinBudget.fresh, intent.deadline - monotonicNow() - pauseDispatchReserve);
      if (freshAllowance <= 0) throw new Error('fresh核验剩余预算不足，停止未发送');
      const refreshed = await waitForPauseIntent(intent, fetchTasks(), freshAllowance,
        '本次启动后的控制记录核验超时，停止未发送');
      const current = tasks.value.find(item => getObjectId(item._id) === b.taskId);
      if (!refreshed || !owned() || !taskMatchesJoinEvidence(current, intent.evidence)) {
        throw new Error(intent.reason || 'fresh任务与本次已确认launch不一致，停止未发送');
      }
      if (intent.deadline - monotonicNow() < pauseDispatchReserve) throw new Error('剩余预算不足以容纳原安全停止路径，停止未发送');
      const launch = intent.evidence.launchAttemptId;
      intent.phase = 'stopping';
      const stopped = await waitForPauseIntent(intent, requestPauseTask(current, {
        refreshAfter: false, joinDeadline: intent.deadline,
        ...(launch === null ? { expectedLaunchAbsent: true } : { expectedLaunchAttemptId: launch }),
        onDispatch: () => { intent.stopSent = true; },
      }), null, '停止结果未确认'); // original performPauseTask owns the remaining UI timer
      if (stopped !== true) throw new Error('停止回执未绑定当前任务；未认定当前任务已暂停');
      if (!disposed) message.success('本次启动结束后暂停已确认（不代表平台请求取消）');
      completion.resolve(true);
    } catch (error) {
      if (!intent.stopSent) {
        if (!disposed) message.warning(`本次暂停未发送：${error instanceof Error ? error.message : String(error)}`);
        completion.resolve(false);
      } else if (intent.cancelled || disposed) {
        if (!disposed) message.warning('停止请求已经发送，最终回执待核验；未宣称停止已撤销');
        completion.resolve(false);
      } else {
        completion.reject(error); // keep original sent-stop failure semantics
      }
    } finally {
      intent.completed = true; intent.rejectWait = null; notice.destroy(); releaseMutation(b.taskId, entry);
    }
  };
  void run();
  return intent.promise;
};

const pauseTask = (task, options) => {
  if (disposed || !allowTaskMutation(task)) return Promise.resolve(false);
  const entry = mutationRequests.value.get(getObjectId(task._id));
  if (entry) {
    if (options?.joinOwnedStart !== false && entry.kind === 'start' && entry.nativeSent && entry.nativeOutcomePromise) return joinOwnStartPause(task, entry);
    message.warning(options?.joinOwnedStart === false
      ? '原任务操作正在进行，自动暂停未发送；未建立新的延后用户意图'
      : '原任务操作正在进行（非已发本次start），本次暂停未发送');
    return Promise.resolve(false);
  }
  return requestPauseTask(task, options);
};

// This only chooses preparation; the native archive rechecks fresh stop/ID/host proof.
const needsArchivePause = (task) =>
  ["running", "launch_unknown", "generating", "generation_unknown", "syncing"].includes(task.status) ||
  taskStopDisplayStatus({ ...task, status: "paused" }, taskStopOperationMap.value[task.name]) !== "paused";

const archiveErrorText = (error) => {
  let text = String(error);
  try {
    const receipt = JSON.parse(text.slice(text.indexOf("{")));
    if (receipt?.command === "delete" && receipt.ok === false) {
      if (receipt.requiredAction === "reconcile_generation_or_sync") return "生成或同步状态尚未收敛，请先核对该操作；暂停不能代替生成/同步确认。";
      if (receipt.requiredAction === "pause") return "停止状态仍未确认，归档未执行；请查看停止详情后重新发起暂停并归档。";
      if (typeof receipt.error === "string") text = receipt.error;
    }
  } catch { /* Original non-JSON error remains diagnostic evidence. */ }
  if (/pause\/reconcile generation or running\/unconfirmed consumer|current valid stop confirmation required/.test(text)) {
    return "停止状态仍未确认，归档未执行；请查看停止详情后重新发起暂停并归档。";
  }
  if (text.includes("candidate generation is not the simulation consumer")) return "生成或同步状态尚未确认，归档未执行；暂停消费者不能代替生成/同步核验。";
  if (text.includes("accepted observation is active/unconfirmed")) return "仍有本地回执观察操作未结束，请先完成该操作再归档；不要求等待全部平台 simulation 终态。";
  return text.length > 400 ? text.slice(0, 400) + "…" : text;
};

const deleteTask = (id) => {
  const taskId = getObjectId(id);
  const original = tasks.value.find(task => getObjectId(task._id) === taskId);
  if (!original) { message.error("任务身份缺失，请刷新列表后再归档。"); return; }
  if (!allowTaskMutation(original)) return;
  const expectedRemote = original?.isRemote === true;
  const confirmation = dialog.warning({
    title: "确认暂停并归档",
    content: "将先暂停并核验停止，再归档任务并移出列表；已有有效停止证明时直接归档。保留候选回执和正式结果，未决 simulation 回执也会保留，不执行平台取消。确定继续？",
    positiveText: "确认",
    negativeText: "取消",
    onPositiveClick: async () => {
      // Confirmation is complete; do not keep the modal open for native work.
      confirmation.destroy();
      try {
        if (!original) throw new Error("任务身份缺失；请刷新列表后再归档");
        if (!allowTaskMutation(tasks.value.find(task => getObjectId(task._id) === taskId))) return;
        await oncePerTask(taskId, async () => {
          const pending = message.loading("正在核验停止状态并归档任务…", { duration: 0 });
          try {
            if (needsArchivePause(original)) {
              const inFlight = [...pendingStops.values()].find(entry => entry.target._id === taskId &&
                entry.target.name === original.name && entry.target.isRemote === expectedRemote);
              const stopped = await (inFlight?.promise ?? requestPauseTask(original, { reportError: false, refreshAfter: false }));
              if (stopped !== true) throw new Error("停止尚未确认，未执行归档。");
            }
            if (disposed) return; // Do not start a second mutation after this view has gone away.
            let archiveReceipt;
            await mutateTaskSource({ fetch: fetchTasks,
              update: async () => { archiveReceipt = await invoke("delete_task", { id: taskId, expectedRemote }); },
              commit: () => {
                const deleted = tasks.value.find((task) => getObjectId(task._id) === taskId);
                if (!disposed && deleted) taskStore.removeTask(deleted.name);
              },
            });
            if (!disposed) {
              const count = archiveReceipt?.unresolvedItemCount;
              if (Number.isSafeInteger(count) && count > 0) {
                message.warning(`任务已归档；${count}条未决或待判定回执已保留，未执行平台取消。`);
              } else {
                message.success("任务已归档，候选回执和正式结果已保留。");
              }
              await fetchTasks();
            }
          } finally { pending.destroy(); }
        }, { joinStop: true });
      } catch (e) {
        console.error("删除任务失败", e);
        if (!disposed) message.error("任务归档未完成：" + archiveErrorText(e));
      }
    },
  });
};

const checkTaskStatus = async (task) => {
  if (taskMutationIssue(task)) return; // This native diagnostic also requires local control registration.
  const target = runtimeTarget(task); // primitives captured before any await
  if (disposed || !matchesRuntimeTarget(tasks.value.find(item => item.name === target.name), target) ||
      runtimeRequests.has(target.name)) return;
  const request = { target, requestId: newTaskRequestId('runtime'), completed: false };
  runtimeRequests.set(target.name, request);
  // Never leave an old positive badge visible while the next probe hangs.
  taskRuntimeStatus.value[target.name] = { ...target, observedAt: Date.now(), state: 'checking',
    stage: 'native_request', detail: '已发起原生消费者检查，等待阶段回报' };
  let timer;
  const ownsRequest = () => !disposed && runtimeRequests.get(target.name) === request &&
    matchesRuntimeTarget(tasks.value.find(item => item.name === target.name), target);
  try {
    const status = await Promise.race([
      invoke("check_task_status", { taskName: target.name, taskId: target.taskId, isRemote: target.isRemote, requestId: request.requestId }),
      new Promise((_, reject) => {
        timer = setTimeout(() => {
          request.completed = true;
          reject(new Error('消费者状态检查超时（15 秒），未收到最终检查结果'));
        }, 15000);
        request.cancelTimer = () => clearTimeout(timer);
      }),
    ]);
    request.completed = true;
    if (ownsRequest()) {
      const result = normalizeTaskRuntime(target, status, Date.now());
      const last = taskRuntimeStatus.value[target.name];
      taskRuntimeStatus.value[target.name] = { ...result, stage: result.stage ?? last?.stage, detail: result.detail ?? last?.detail };
    }
  } catch (err) {
    request.completed = true;
    if (ownsRequest()) {
      const last = taskRuntimeStatus.value[target.name];
      const result = normalizeTaskRuntime(target, null, Date.now(), err);
      taskRuntimeStatus.value[target.name] = { ...result, stage: result.stage ?? last?.stage,
        detail: result.stage && result.stage !== last?.stage ? '原生桥在该阶段返回错误（完整原因见下文）' : last?.detail };
    }
  } finally {
    clearTimeout(timer);
    if (runtimeRequests.get(target.name) === request) runtimeRequests.delete(target.name);
  }
};

// Invalidate on removal, replacement, target switch or leaving an observed phase
// (except terminal: retain evidence of the same still-live consumer). This
// also prevents an away-and-back transition from reviving an old async result.
const stopRuntimeWatch = watch(tasks, () => {
  for (const [id, entry] of mutationRequests.value) {
    if (entry.kind === 'start') rememberStartSnapshot(entry, tasks.value.find(task => getObjectId(task._id) === id));
  }
  const modal = startModalTarget.value;
  if (modal && !matchesStartModalTarget(tasks.value.find(task => task.name === modal.name), modal)) {
    startModalTarget.value = null;
    showStartModal.value = false;
    selectedTask.value = null;
  }
  for (const [name, observation] of Object.entries(taskRuntimeStatus.value)) {
    if (!matchesRuntimeTarget(tasks.value.find(item => item.name === name), observation)) invalidateRuntime(name);
  }
  for (const [name, request] of runtimeRequests) {
    if (!matchesRuntimeTarget(tasks.value.find(item => item.name === name), request.target)) invalidateRuntime(name);
  }
}, { deep: true, flush: 'sync' });

// --- Modal Handlers ---
const handleAddTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting" } });
    message.success("任务创建成功");
    fetchTasks();
  } catch (err) {
    console.error("添加任务失败：", err);
    message.error("任务创建失败：" + String(err));
  }
};

const handleAddSuperTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting", task_type: "super" } });
    message.success("Super任务创建成功");
    fetchTasks();
  } catch (err) {
    console.error("添加Super任务失败：", err);
    message.error("Super任务创建失败：" + String(err));
  }
};

const handleAddPriorityTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting", task_type: "priority" } });
    message.success("Priority任务创建成功");
    fetchTasks();
  } catch (err) {
    console.error("添加Priority任务失败：", err);
    message.error("Priority任务创建失败：" + String(err));
  }
};

const startTask = async (command, { taskName, config, isRemote }) => {
  const original = tasks.value.find((task) => task.name === taskName);
  const target = original && { _id: getObjectId(original._id), name: taskName, isRemote: original.isRemote === true };
  let outcomeTarget;
  try {
    if (!target?._id) throw new Error("任务身份缺失；请刷新任务列表后再启动");
    if (!allowTaskMutation(original)) return;
    if (isRemote !== target.isRemote) throw new Error("启动参数不能切换实际执行端；请使用原任务的安全同步/目标意向流程，已有任务不会自动迁移");
    // Forward saved creation intent, not a new checkbox migration. Only core
    // may approve pristine Priority destination or require initial sync; GUI
    // never interprets status/history/receipts to grant a host transition.
    const modal = startModalTarget.value;
    if (!modal || !matchesStartModalTarget(original, modal)) {
      throw new Error("启动弹窗对应的任务身份/执行端/启动批次已变化，请重新核验原任务");
    }
    const blocked = startBlockReason(original);
    if (blocked) throw new Error(blocked);
    // REGULAR/SUPER always resume on their actual execution host. Only
    // Priority retains the original saved first-destination intent contract.
    const requestedRemote = original.taskType === "priority" && typeof original.targetRemote === "boolean"
      ? original.targetRemote : target.isRemote;
    outcomeTarget = { taskId: target._id, name: taskName, remote: requestedRemote, actualRemote: target.isRemote };
    const args = { taskName, taskId: target._id, ...(config === undefined ? {} : { config: JSON.stringify(config) }),
      isRemote: requestedRemote, expectedRemote: target.isRemote };
    await oncePerTask(target._id, async (entry) => {
      // Consume this modal intent before native launch; failure/unknown cannot
      // be resubmitted from the stale modal without a new original check.
      startModalTarget.value = null;
      showStartModal.value = false;
      invalidateRuntime(taskName);
      let outcome;
      await mutateTaskSource({ fetch: fetchTasks, update: async () => {
        entry.nativeOutcomePromise = Promise.resolve(invoke(command, args));
        entry.nativeSent = true;
        outcome = taskStartOutcome(await entry.nativeOutcomePromise, outcomeTarget);
        // Defensive against a mismatched adapter resolving a failed TaskOutcome.
        if (outcome?.ok === false) throw outcome;
      } });
      if (!disposed) {
        if (outcome?.outcome === 'already_running') {
          message.success("原任务已运行，继续查看原任务；未新启动、未应用新配置");
        } else {
          if (!entry.invalidated) taskStore.clearTaskStop(target);
          message.success("任务启动已确认（不代表平台完成）");
        }
        if (!entry.pauseIntent || entry.pauseIntent.completed) fetchTasks();
      }
    }, { kind: 'start', binding: { ...outcomeTarget, taskType: original.taskType ?? 'regular',
      authProfile: original.auth_profile ?? 'user1', launchAttemptId: taskLaunchValue(original) } });
  } catch (err) {
    console.error("启动任务失败", err);
    if (!disposed) {
      const outcome = outcomeTarget && taskStartFailure(err, outcomeTarget);
      if (outcome?.outcome === 'already_running' && outcome.consumerRunning === true) {
        message.warning(`原消费者仍在运行，本次配置未应用（${outcome.configComparison === 'different' ? '配置不同' : '配置未核验'}）；继续查看原任务。${outcome.error || ''}`);
      } else {
        message.error(`任务启动失败：${outcome?.error || (err instanceof Error ? err.message : String(err))}`);
      }
    }
  }
};
const handleStartTask = (args) => startTask("start_task", args);
const handleStartSuperTask = (args) => startTask("start_super_task", args);
const handlePriorityStartTask = (args) => startTask("start_priority_task", args);

const handleUpdateTask = async ({ task, config, isRemote }) => {
  try {
    if (!allowTaskMutation(task) || !allowTaskMutation(tasks.value.find(item => getObjectId(item._id) === getObjectId(task._id)))) return;
    await oncePerTask(getObjectId(task._id), async () => {
    await invoke("update_task", {
      id: getObjectId(task._id),
      updates: { config: JSON.stringify(config), is_remote: isRemote },
    });
    message.success("任务更新成功");
    showUpdateModal.value = false;
    fetchTasks();
    }, { kind: 'update' });
  } catch (err) {
    console.error("更新任务失败", err);
    message.error("任务更新失败");
  }
};

const handleUpdatePriorityTask = async ({ taskName, config, isRemote }) => {
  try {
    const task = tasks.value.find((task) => task.name === taskName);
    if (!task) throw new Error("任务身份缺失；请刷新任务列表");
    if (!allowTaskMutation(task)) return;
    if (isRemote !== (task.isRemote === true)) throw new Error("配置更新不能迁移执行端");
    await oncePerTask(getObjectId(task._id), async () => {
    await invoke("update_priority_task", { taskName, taskId: getObjectId(task._id), config: JSON.stringify(config), isRemote });
    message.success("Priority任务更新成功");
    showUpdateModal.value = false;
    fetchTasks();
    }, { kind: 'update' });
  } catch (err) {
    console.error("更新 Priority 任务失败", err);
    message.error("Priority任务更新失败：" + String(err));
  }
};

// --- Helper Functions ---
const detailTaskId = computed(() => typeof route.query?.taskId === 'string' ? route.query.taskId : '');
const detailTask = computed(() => tasks.value.find(task => getObjectId(task._id) === detailTaskId.value) ?? null);
const viewTask = (id) => router.push({ name: 'TaskManager', query: { ...route.query, taskId: getObjectId(id) } });
const closeTaskDetails = () => {
  const { taskId, ...query } = route.query ?? {};
  return router.replace({ name: 'TaskManager', query });
};
const getIsSuper = (task) => task && task.taskType === "super";
const getIsPriority = (task) => task && task.taskType === "priority";

const openStartModal = (task) => {
  const current = tasks.value.find(item => getObjectId(item._id) === getObjectId(task._id));
  const target = { taskId: getObjectId(task._id), name: task.name, isRemote: task.isRemote === true,
    authProfile: task.auth_profile ?? 'user1', taskType: task.taskType ?? 'regular', targetRemote: task.targetRemote ?? null };
  if (disposed || !matchesStartModalTarget(current, target)) return;
  const blocked = startBlockReason(current);
  if (blocked) { message.warning(blocked); return; }
  startModalTarget.value = target;
  selectedTask.value = { ...current };
  showStartModal.value = true;
};

const openUpdateModal = (task) => {
  if (!allowTaskMutation(task)) return;
  selectedTaskForUpdate.value = task;
  isSuperForUpdate.value = getIsSuper(task);
  isPriorityForUpdate.value = getIsPriority(task);
  showUpdateModal.value = true;
};

// --- Lifecycle Hooks ---
// Progress is display-only: counts do not establish lifecycle completion or
// authorize a stop. Existing lifecycle owners supply task state; pause stays explicit.
// No periodic process discovery or GUI completion timer.
const progressLifecycle = createTaskProgressLifecycle({
  listen,
  ready: () => invoke("frontend_ready"),
  fetch: fetchTasks,
  onProgress: handleTaskProgressUpdate,
  onError: handleTaskProgressError,
  onRefreshError: handleTaskProgressRefreshError,
});

let registryUnlisten;
let operationUnlisten;
onMounted(async () => {
  try {
    const stageCleanup = await listen('task-operation-stage', ({ payload }) => handleTaskOperationStage(payload));
    if (disposed) { stageCleanup(); return; }
    operationUnlisten = stageCleanup;
    const unlisten = await listen('task-registry-changed', () => {
      if (!disposed) fetchTasks();
    });
    if (disposed) { unlisten(); return; }
    registryUnlisten = unlisten;
    await progressLifecycle.start();
  } catch (error) {
    operationUnlisten?.();
    operationUnlisten = undefined;
    registryUnlisten?.();
    registryUnlisten = undefined;
    console.error("启动任务进度监听失败", error);
    if (!disposed) message.error("启动任务进度监听失败");
  }
});

onUnmounted(() => {
  disposed = true;
  for (const entry of mutationRequests.value.values()) cancelPauseIntent(entry.pauseIntent, '页面已离开，不再派发后续停止');
  mutationRequests.value.clear();
  operationUnlisten?.();
  operationUnlisten = undefined;
  registryUnlisten?.();
  registryUnlisten = undefined;
  for (const { target, operation, cancelTimer } of pendingStops.values()) {
    cancelTimer?.();
    taskStore.markTaskStopUnconfirmed(target, operation, '页面已离开，停止回执未完成确认', 'client_disposed');
  }
  pendingStops.clear();
  for (const request of runtimeRequests.values()) request.cancelTimer?.();
  runtimeRequests.clear();
  stopRuntimeWatch();
  progressLifecycle.stop();
});

watch(() => route.fullPath, () => fetchTasks());
</script>
