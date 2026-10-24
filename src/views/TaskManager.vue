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
            <PriorityTaskCard
              :task="task"
              :progress="taskProgressMap[task.name]"
              :task-runtime-status="taskRuntimeStatus[task.name]"
              :stop-operation="taskStopOperationMap[task.name]"
              @update:is-remote="(val) => updateIsRemote(task._id, val)"
              @start="openStartModal(task)"
              @update="openUpdateModal(task)"
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
            <RegularTaskCard
              :task="task"
              :generation-busy="Boolean(generationRequests[getObjectId(task._id)])"
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
import { useTaskStore, validStopReceipt } from "../stores/taskStore";
import { createTaskFetcher, createTaskProgressLifecycle, createAutoPauseScheduler, syncTaskToRemote, mutateTaskSource } from "../utils/taskProgress.js";
import { newTaskRequestId, validOperationStage } from '../utils/taskDiagnostics.js';
import { canGenerate } from '../utils/taskGeneration.js';
import { storeToRefs } from "pinia";
import { useMessage, useDialog } from "naive-ui";

// Import new components
import TaskManagerToolbar from "../components/TaskCards/TaskManagerToolbar.vue";
import PriorityTaskCard from "../components/TaskCards/PriorityTaskCard.vue";
import RegularTaskCard from "../components/TaskCards/RegularTaskCard.vue";

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
const runtimeTarget = (task) => ({ taskId: getObjectId(task._id), name: task.name, isRemote: task.isRemote === true, authProfile: task.auth_profile ?? "user1", launchAttemptId: task.launchAttemptId ?? null });
const matchesRuntimeTarget = (task, target) => task?.status === 'running' &&
  typeof target.taskId === 'string' && !!target.taskId && getObjectId(task._id) === target.taskId &&
  task.name === target.name && (task.isRemote === true) === target.isRemote &&
  (task.auth_profile ?? "user1") === target.authProfile && (task.launchAttemptId ?? null) === (target.launchAttemptId ?? null);
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

const generationRequests = ref({});
const generateList = async (id) => {
  const key = getObjectId(id);
  const task = tasks.value.find(item => getObjectId(item._id) === key);
  if (!canGenerate(task, Boolean(generationRequests.value[key]))) return;
  generationRequests.value[key] = true; // disable immediately, before awaiting native
  try {
    await invoke("generate_list", { id: key });
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

// Low-level update deliberately throws so a sync failure cannot be swallowed.
const setRemoteSource = (task, isRemote) => mutateTaskSource({
  fetch: fetchTasks,
  update: () => invoke("update_remote_status", {
    id: getObjectId(task._id),
    updates: { is_remote: isRemote },
  }),
  commit: () => {
    const current = tasks.value.find((item) => item.name === task.name);
    if (!disposed && current && getObjectId(current._id) === getObjectId(task._id)) {
      taskStore.setTaskSource(task.name, isRemote);
    }
  },
});

const syncRemoteTask = async (taskName) => {
  try {
    const targetTask = tasks.value.find((task) => task.name === taskName);
    if (!targetTask) throw new Error("任务列表缺少所选任务记录");
    await syncTaskToRemote({
      sync: () => invoke("sync_remote_task", { alphaMissionList: taskName }),
      updateSource: () => setRemoteSource(targetTask, true),
      refresh: () => {
        if (!disposed) message.success("任务同步成功，运行方式已更新为远程");
        return fetchTasks();
      },
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
    await setRemoteSource(targetTask, isRemote);
    if (!disposed) message.success(`运行方式已更新为 ${isRemote ? "远程" : "本地"}`);
    await fetchTasks();
  } catch (err) {
    console.error("更新 isRemote 失败", err);
    if (!disposed) message.error("运行方式更新失败: " + err);
  }
};

const pauseTask = async (task) => {
  // Capture before awaiting: the current task/host may be replaced meanwhile.
  const target = { _id: getObjectId(task._id), name: task.name, isRemote: task.isRemote === true };
  if (disposed) return false;
  const operation = taskStore.beginTaskStop(target);
  if (!operation) return false; // missing/replaced task or an already pending stop
  const pending = { target, operation, completed: false, failureSource: 'native' };
  pendingStops.set(operation.generation, pending);
  let receipt;
  let timer;
  try {
    // Reuse both invalidation barriers. A resolved void command or stdout text
    // is not proof: native must verify process exit AND commit control state.
    await mutateTaskSource({
      fetch: fetchTasks,
      update: async () => {
        try {
          receipt = await Promise.race([
            invoke("pause_task", { taskName: target.name, taskId: target._id, isRemote: target.isRemote, requestId: operation.requestId }),
            new Promise((_, reject) => {
              timer = setTimeout(() => {
                pending.completed = true;
                pending.failureSource = 'client_timeout';
                reject(new Error("停止确认超时（120 秒）；最终暂停回执待核对"));
              }, 120000);
              pending.cancelTimer = () => clearTimeout(timer);
            }),
          ]);
        } finally { pending.completed = true; clearTimeout(timer); }
        if (!validStopReceipt(target, receipt)) {
          pending.failureSource = 'receipt_validation';
          throw new Error("暂停回执校验失败：回执字段、任务身份或执行主机有差异");
        }
      },
      commit: () => { if (!disposed) taskStore.confirmTaskPaused(target, receipt, operation); },
    });
  } catch (error) {
    if (!disposed) {
      taskStore.markTaskStopUnconfirmed(target, operation, error, pending.failureSource);
      message.error("停止操作失败：" + String(error));
    }
    throw error;
  } finally { pendingStops.delete(operation.generation); }
  if (!disposed) await fetchTasks();
  return true;
};

const deleteTask = (id) => {
  dialog.warning({
    title: "确认删除",
    content: "删除任务后将无法恢复，确定要继续吗？",
    positiveText: "确认",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await invoke("delete_task", { id: getObjectId(id) });
        if (!disposed) {
          const deleted = tasks.value.find((task) => getObjectId(task._id) === getObjectId(id));
          if (deleted) taskStore.removeTask(deleted.name);
          await fetchTasks();
        }
      } catch (e) {
        console.error("删除任务失败", e);
        message.error("删除任务失败");
      }
    },
  });
};

const checkTaskStatus = async (task) => {
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

// Invalidate on removal, replacement, target switch or leaving running. This
// also prevents an away-and-back transition from reviving an old async result.
const stopRuntimeWatch = watch(tasks, () => {
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
    message.error("任务创建失败");
  }
};

const handleAddSuperTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting", task_type: "super" } });
    message.success("Super任务创建成功");
    fetchTasks();
  } catch (err) {
    console.error("添加Super任务失败：", err);
    message.error("Super任务创建失败");
  }
};

const handleAddPriorityTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting", task_type: "priority" } });
    message.success("Priority任务创建成功");
    fetchTasks();
  } catch (err) {
    console.error("添加Priority任务失败：", err);
    message.error("Priority任务创建失败");
  }
};

const handleStartTask = async ({ taskName, config, isRemote }) => {
  const original = tasks.value.find((task) => task.name === taskName);
  const target = original && { _id: getObjectId(original._id), name: taskName, isRemote: original.isRemote === true };
  try {
    if (!target?._id) throw new Error("任务身份缺失；请刷新任务列表后再启动");
    invalidateRuntime(taskName);
    await invoke("start_task", { taskName, taskId: target._id, config: JSON.stringify(config), isRemote });
    if (!disposed && target) taskStore.clearTaskStop(target);
    message.success("任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动任务失败", err);
    message.error(`任务启动失败：${err instanceof Error ? err.message : String(err)}`);
  }
};

const handleStartSuperTask = async ({ taskName, config, isRemote }) => {
  const original = tasks.value.find((task) => task.name === taskName);
  const target = original && { _id: getObjectId(original._id), name: taskName, isRemote: original.isRemote === true };
  try {
    if (!target?._id) throw new Error("任务身份缺失；请刷新任务列表后再启动");
    invalidateRuntime(taskName);
    await invoke("start_super_task", { taskName, taskId: target._id, config: JSON.stringify(config), isRemote });
    if (!disposed && target) taskStore.clearTaskStop(target);
    message.success("Super 任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动 Super 任务失败", err);
    message.error(`Super 任务启动失败：${err instanceof Error ? err.message : String(err)}`);
  }
};

const handlePriorityStartTask = async ({ taskName, config, isRemote }) => {
  const original = tasks.value.find((task) => task.name === taskName);
  const target = original && { _id: getObjectId(original._id), name: taskName, isRemote: original.isRemote === true };
  try {
    if (!target?._id) throw new Error("任务身份缺失；请刷新任务列表后再启动");
    invalidateRuntime(taskName);
    await invoke("start_priority_task", { taskName, taskId: target._id, config: JSON.stringify(config), isRemote });
    if (!disposed && target) taskStore.clearTaskStop(target);
    message.success("任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动任务失败", err);
    message.error(`任务启动失败：${err instanceof Error ? err.message : String(err)}`);
  }
};

const handleUpdateTask = async ({ task, config, isRemote }) => {
  try {
    await invoke("update_task", {
      id: getObjectId(task._id),
      updates: { config: JSON.stringify(config), is_remote: isRemote },
    });
    message.success("任务更新成功");
    showUpdateModal.value = false;
    fetchTasks();
  } catch (err) {
    console.error("更新任务失败", err);
    message.error("任务更新失败");
  }
};

const handleUpdatePriorityTask = async ({ taskName, config, isRemote }) => {
  try {
    await invoke("update_priority_task", { taskName, config: JSON.stringify(config), isRemote });
    message.success("Priority任务更新成功");
    showUpdateModal.value = false;
    fetchTasks();
  } catch (err) {
    console.error("更新 Priority 任务失败", err);
    message.error("Priority任务更新失败");
  }
};

// --- Helper Functions ---
const viewTask = (id) => router.push(`/task/${getObjectId(id)}`);
const getIsSuper = (task) => task && task.taskType === "super";
const getIsPriority = (task) => task && task.taskType === "priority";

const openStartModal = (task) => {
  selectedTask.value = task;
  showStartModal.value = true;
};

const openUpdateModal = (task) => {
  selectedTaskForUpdate.value = task;
  isSuperForUpdate.value = getIsSuper(task);
  isPriorityForUpdate.value = getIsPriority(task);
  showUpdateModal.value = true;
};

// --- Lifecycle Hooks ---
const pollTaskStatus = () => {
  if (disposed) return;
  tasks.value.forEach((task) => {
    if (task.status === "running") checkTaskStatus(task);
  });
};

const autoPause = createAutoPauseScheduler({
  getTasks: () => tasks.value,
  getProgress: () => taskProgressMap.value,
  pause: (task) => {
    // Progress recovery must not blindly retry a failed/unconfirmed stop.
    if (taskStopOperationMap.value[task.name]) return;
    return pauseTask(task);
  },
  onError: (error) => {
    console.error("自动暂停失败", error);
    message.error("自动暂停失败: " + error);
  },
});
const stopAutoPauseWatch = watch([tasks, taskProgressMap], () => autoPause.reconcile(), {
  deep: true,
  flush: "sync",
});

const progressLifecycle = createTaskProgressLifecycle({
  listen,
  ready: () => invoke("frontend_ready"),
  fetch: fetchTasks,
  poll: pollTaskStatus,
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
    if (!disposed) autoPause.reconcile();
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
  stopAutoPauseWatch();
  autoPause.stop();
});

watch(() => route.fullPath, () => fetchTasks());
</script>
