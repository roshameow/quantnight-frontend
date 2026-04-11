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
const unlisten = ref(null);

// Computed properties for task lists
const taskIsEmpty = computed(() => tasks.value.length === 0);
const priorityTasks = computed(() => (tasks.value || []).filter((t) => t.taskType === "priority"));
const otherTasks = computed(() => (tasks.value || []).filter((t) => t.taskType !== "priority"));

// --- Data Fetching and Store Management ---
const fetchTasks = async () => {
  try {
    const result = await invoke("get_all_tasks");
    const activeTasks = result.filter((task) => task.status !== "deactive");
    taskStore.clearTasks();
    activeTasks.forEach((task) => taskStore.addTask(task));
    taskStore.initializeProgressForTasks(activeTasks);
  } catch (e) {
    console.error("加载任务失败", e);
    message.error("加载任务失败");
  }
};

// --- Backend Event Handling ---
const handleTaskProgressUpdate = (progress) => {
  if (progress && typeof progress === 'object' && 'collection' in progress) {
    taskStore.updateProgress(progress);
    const { collection, success, total } = progress;
    if (success >= total && total > 0) {
      const finishedTask = (tasks.value || []).find((t) => t.name === collection);
      if (finishedTask && finishedTask.status === "running") {
        console.log(`任务 ${collection} 已完成，5 分钟后自动暂停`);
        setTimeout(() => {
          const latestTask = (tasks.value || []).find((t) => t.name === collection);
          if (latestTask && latestTask.status === "running") {
            pauseTask(latestTask);
            console.log(`任务 ${collection} 已在 5 分钟后自动暂停`);
          }
        }, 5 * 60 * 1000);
      }
    }
  } else {
    console.warn("收到格式不正确的 task-progress-update payload:", progress);
  }
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

const syncRemoteTask = async (taskName) => {
  try {
    await invoke("sync_remote_task", { alphaMissionList: taskName });
    const targetTask = tasks.value.find((t) => t.name === taskName);
    if (targetTask) {
      await updateIsRemote(targetTask._id, true);
    }
    fetchTasks();
  } catch (err) {
    console.error("同步失败", err);
    message.error("任务同步失败");
  }
};

const updateIsRemote = async (id, isRemote) => {
  try {
    await invoke("update_remote_status", {
      id: getObjectId(id),
      updates: { is_remote: isRemote },
    });
    message.success(`运行方式已更新为 ${isRemote ? "远程" : "本地"}`);
    // 重新获取任务数据以更新界面
    await fetchTasks();
  } catch (err) {
    console.error("更新 isRemote 失败", err);
    message.error("运行方式更新失败");
  }
};

const pauseTask = async (task) => {
  await invoke("pause_task", { taskName: task.name, isRemote: task.isRemote });
  fetchTasks();
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
        await fetchTasks();
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
    taskRuntimeStatus.value[task.name] = status.trim();
  } catch (err) {
    console.error(`检查任务 ${task.name} 状态失败`, err);
    taskRuntimeStatus.value[task.name] = "error";
  }
};

// --- Modal Handlers ---
const handleAddTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting" } });
    await fetchTasks();
    message.success("任务创建成功");
  } catch (err) {
    console.error("添加任务失败：", err);
    message.error("任务创建失败");
  }
};

const handleAddSuperTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting", task_type: "super" } });
    await fetchTasks();
    message.success("Super任务创建成功");
  } catch (err) {
    console.error("添加Super任务失败：", err);
    message.error("Super任务创建失败");
  }
};

const handleAddPriorityTask = async (task) => {
  try {
    await invoke("create_task", { newTask: { ...task, status: task.status ?? "waiting", task_type: "priority" } });
    await fetchTasks();
    message.success("Priority任务创建成功");
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
onMounted(async () => {
  await fetchTasks();
  unlisten.value = await listen("task-progress-update", (event) => handleTaskProgressUpdate(event.payload));
  await invoke("frontend_ready");
  pollTaskStatus();
});

onUnmounted(() => {
  if (unlisten.value) unlisten.value();
});

watch(() => route.fullPath, () => fetchTasks());

// Polling for priority tasks
const pollTaskStatus = () => {
  priorityTasks.value.forEach((task) => {
    if (task.status === "running") checkTaskStatus(task);
  });
};
setInterval(pollTaskStatus, 600000); // 10 minutes
</script>
