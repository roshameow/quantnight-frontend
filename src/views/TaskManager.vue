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
              :progress="taskProgressMap[task.name]"
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

<script setup>
import { ref, onMounted, onUnmounted, watch, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useRoute, useRouter } from "vue-router";
import { useTaskStore } from "../stores/taskStore";
import { createTaskFetcher, createTaskProgressLifecycle, createAutoPauseScheduler, syncTaskToRemote, mutateTaskSource } from "../utils/taskProgress.js";
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
const { tasks, taskProgressMap } = storeToRefs(taskStore);
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

// --- Task Actions (invoking Rust backend) ---
const getObjectId = (id) => (typeof id === "object" && "$oid" in id ? id.$oid : id);

const generateList = async (id) => {
  try {
    await invoke("generate_list", { id: getObjectId(id) });
    message.success("生成列表任务已提交");
    fetchTasks();
  } catch (err) {
    console.error("生成列表失败", err);
    message.error("生成列表失败: " + err);
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
    if (!targetTask) throw new Error("任务不存在");
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
    if (!targetTask) throw new Error("任务不存在");
    await setRemoteSource(targetTask, isRemote);
    if (!disposed) message.success(`运行方式已更新为 ${isRemote ? "远程" : "本地"}`);
    await fetchTasks();
  } catch (err) {
    console.error("更新 isRemote 失败", err);
    if (!disposed) message.error("运行方式更新失败: " + err);
  }
};

const pauseTask = async (task) => {
  await invoke("pause_task", { taskName: task.name, isRemote: task.isRemote });
  await fetchTasks();
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
  try {
    const status = await invoke("check_task_status", {
      taskName: task.name,
      isRemote: task.isRemote,
    });
    const current = tasks.value.find((item) => item.name === task.name);
    if (!disposed && current?.status === "running" && current.isRemote === task.isRemote) {
      taskRuntimeStatus.value[task.name] = status.trim();
    }
  } catch (err) {
    console.error(`检查任务 ${task.name} 状态失败`, err);
    const current = tasks.value.find((item) => item.name === task.name);
    if (!disposed && current?.status === "running" && current.isRemote === task.isRemote) {
      taskRuntimeStatus.value[task.name] = "error";
    }
  }
};

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
  try {
    await invoke("start_task", { taskName, config: JSON.stringify(config), isRemote });
    message.success("任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动任务失败", err);
    message.error("任务启动失败");
  }
};

const handleStartSuperTask = async ({ taskName, config, isRemote }) => {
  try {
    await invoke("start_super_task", { taskName, config: JSON.stringify(config), isRemote });
    message.success("Super 任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动 Super 任务失败", err);
    message.error("Super 任务启动失败");
  }
};

const handlePriorityStartTask = async ({ taskName, config, isRemote }) => {
  try {
    await invoke("start_priority_task", { taskName, config: JSON.stringify(config), isRemote });
    message.success("任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动任务失败", err);
    message.error("任务启动失败");
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
  priorityTasks.value.forEach((task) => {
    if (task.status === "running") checkTaskStatus(task);
  });
};

const autoPause = createAutoPauseScheduler({
  getTasks: () => tasks.value,
  getProgress: () => taskProgressMap.value,
  pause: pauseTask,
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

onMounted(async () => {
  try {
    await progressLifecycle.start();
    if (!disposed) autoPause.reconcile();
  } catch (error) {
    console.error("启动任务进度监听失败", error);
    if (!disposed) message.error("启动任务进度监听失败");
  }
});

onUnmounted(() => {
  disposed = true;
  progressLifecycle.stop();
  stopAutoPauseWatch();
  autoPause.stop();
});

watch(() => route.fullPath, () => fetchTasks());
</script>
