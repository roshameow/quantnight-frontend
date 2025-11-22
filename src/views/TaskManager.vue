<template>
  <div style="padding: 0px">
    <n-page-header title="回测任务管理" style="margin-bottom: 20px">
      <template #extra>
        <div style="display: flex; gap: 12px">
          <n-button
            type="primary"
            size="large"
            @click="showAddPriorityModal = true"
            style="
              border-radius: 20px;
              padding: 10px 10px;
              font-size: 14px;
              background-color: #ff9800;
            "
          >
            ➕ 添加Priority任务
          </n-button>
          <n-button
            type="primary"
            size="large"
            @click="showAddModal = true"
            style="
              border-radius: 20px;
              padding: 10px 10px;
              font-size: 14px;
              background-color: #4caf50;
            "
          >
            ➕ 添加任务
          </n-button>
          <n-button
            type="primary"
            size="large"
            @click="showAddSuperModal = true"
            style="
              border-radius: 20px;
              padding: 10px 10px;
              font-size: 14px;
              background-color: #2196f3;
            "
          >
            ➕ 添加Super任务
          </n-button>
        </div>
      </template>
    </n-page-header>

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
            <n-card
              embedded
              size="small"
              :segmented="true"
              class="task-card priority-task"
              style="margin: auto"
            >
              <template #header>
                <div class="card-header">
                  <div class="header-content">
                    <span class="task-name">{{ task.name }}</span>
                    <n-tag :type="getStatusType(task.status)" size="small" round>
                      {{ task.status }}
                    </n-tag>
                  </div>
                </div>
              </template>

              <div>
                任务状态：
                {{ taskRuntimeStatus[task.name] ?? "未知" }}
              </div>
              <div
                v-if="task.status === 'running' && getPriorityConfig(task)"
                style="margin-top: 4px; font-size: 12px; color: #555"
              >
                优先级: {{ getPriorityConfig(task).priority ?? "-" }} &nbsp;|&nbsp; Filter
                Params: {{ getPriorityConfig(task).filter_params ?? "-" }}
              </div>

              <n-switch
                v-model:value="task.isRemote"
                :checked-value="true"
                :unchecked-value="false"
                @update:value="(val) => updateIsRemote(task._id, val)"
              >
                <template #checked>远程</template>
                <template #unchecked>本地</template>
              </n-switch>
              <n-space style="margin-top: 10px" justify="center">
                <n-button size="small" type="success" @click="openStartModal(task)">
                  <template #icon>>></template>启动
                </n-button>
                <n-button size="small" @click="openUpdateModal(task)">
                  <template #icon>↻</template>更新
                </n-button>
                <n-button size="small" type="warning" @click="pauseTask(task)">
                  <template #icon>■</template>停止
                </n-button>
                <n-button size="small" type="error" @click="deleteTask(task._id)">
                  <template #icon>x</template>删除
                </n-button>
              </n-space>
            </n-card>
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
                  <div class="progress-background" :style="getProgressBarStyle(task)" />
                  <div class="header-content">
                    <span class="task-name">{{ task.name }}</span>
                    <n-tag :type="getStatusType(task.status)" size="small" round>
                      {{ task.status }}
                    </n-tag>
                  </div>
                </div>
              </template>
              <div v-if="taskProgressMap[task.name]">
                <div>
                  任务进度：
                  {{
                    (taskProgressMap[task.name]?.success ?? 0) -
                    (taskProgressMap[task.name]?.priority_success ?? 0)
                  }}+
                  {{ taskProgressMap[task.name]?.priority_success ?? 0 }}
                  /
                  {{
                    (taskProgressMap[task.name]?.total ?? 0) -
                    (taskProgressMap[task.name]?.priority_total ?? 0)
                  }}+
                  {{ taskProgressMap[task.name]?.priority_total ?? 0 }}
                </div>
              </div>
              <div v-if="task.status === 'running'">
                <template v-if="hasConcurrencyConfig(task)">
                  <div style="margin-top: 4px; font-size: 12px; color: #555">
                    最大并发: {{ getMaxConcurrent(task) }} &nbsp;|&nbsp; 子任务上限:
                    {{ getMaxMultiSimulationChildren(task) }}
                  </div>
                </template>
              </div>
              <n-space style="margin-top: 2px">
                <n-switch
                  v-model:value="task.isRemote"
                  :checked-value="true"
                  :unchecked-value="false"
                  @update:value="(val) => updateIsRemote(task._id, val)"
                >
                  <template #checked>远程</template>
                  <template #unchecked>本地</template>
                </n-switch>
                <n-space wrap size="small" style="margin-top: 10px">
                  <n-button size="small" @click="viewTask(task._id)">
                    <template #icon>[ ]</template>查看
                  </n-button>
                  <n-button
                    size="small"
                    @click="generateList(task._id)"
                    :disabled="task.status === 'running'"
                  >
                    <template #icon>=</template>生成list
                  </n-button>
                  <n-button
                    size="small"
                    @click="syncRemoteTask(task.name)"
                    :disabled="task.status === 'running'"
                  >
                    <template #icon>-></template>发送到远程
                  </n-button>
                  <n-button size="small" @click="openStartModal(task)">
                    <template #icon>>></template>启动
                  </n-button>
                  <n-button size="small" @click="pauseTask(task)">
                    <template #icon>| |</template>暂停
                  </n-button>
                  <n-button size="small" type="error" @click="deleteTask(task._id)">
                    <template #icon>x</template>删除
                  </n-button>
                </n-space>
              </n-space>
            </n-card>
          </n-gi>
        </n-grid>
      </n-collapse-item>
    </n-collapse>

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
      @start-super-task="handleStartTask"
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
import { ref, onMounted, onUnmounted, watch } from "vue";
import { toRaw } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useRoute, useRouter } from "vue-router"; // ✅ 确保 useRoute 导入
import { useTaskStore } from "../stores/taskStore";
import AddTaskModal from "../views/AddTaskModal.vue";
import AddSuperTaskModal from "../views/AddSuperTaskModal.vue";
import StartTaskModal from "../views/StartTaskModal.vue";
import UpdateTaskModal from "../views/UpdateTaskModal.vue";
import AddPriorityTaskModal from "../views/AddPriorityTaskModal.vue";
import { storeToRefs } from "pinia";

const taskStore = useTaskStore();
const { tasks, taskProgressMap } = storeToRefs(taskStore); // 解构出来

const taskIsEmpty = computed(() => tasks.value.length === 0);

const fetchTasks = async () => {
  try {
    const result = await invoke("get_all_tasks");
    const activeTasks = result.filter((task) => task.status !== "deactive");

    // 使用任务列表更新进度和添加任务
    taskStore.clearTasks(); // 清空现有任务
    activeTasks.forEach((task) => taskStore.addTask(task));
    taskStore.initializeProgressForTasks(activeTasks);
  } catch (e) {
    console.error("加载任务失败", e);
  }
};

const handleTaskProgressUpdate = (progress) => {
  taskStore.updateProgress(progress);
};

const showAddPriorityModal = ref(false);

import { useMessage } from "naive-ui";
import { computed } from "vue";

// const tasks = computed(() => taskStore.tasks.value);

const priorityTasks = computed(() =>
  (tasks.value || []).filter((t) => t.taskType === "priority")
);

const otherTasks = computed(() =>
  (tasks.value || []).filter((t) => t.taskType !== "priority")
);

const message = useMessage(); // 在 setup() 中获取消息方法

const showAddModal = ref(false);
const showAddSuperModal = ref(false);
const showStartModal = ref(false);
const selectedTask = ref(null); // 默认 null，也可以初始化成一个空对象
const showUpdateModal = ref(false);
const selectedTaskForUpdate = ref(null);

const isSuperForUpdate = ref(false);
const isPriorityForUpdate = ref(false);

// 存储 tmux 中运行状态
const taskRuntimeStatus = ref({});

// 更新任务的提交逻辑
// 普通 / super 任务
const handleUpdateTask = async ({ task, config, isRemote }) => {
  try {
    await invoke("update_task", {
      id: task._id?.$oid ?? task._id,
      updates: {
        config: JSON.stringify(config),
        is_remote: isRemote,
      },
    });
    message.success("任务更新成功");
    showUpdateModal.value = false;
    fetchTasks();
  } catch (err) {
    console.error("更新任务失败", err);
    message.error("任务更新失败");
  }
};

// Priority 任务
const handleUpdatePriorityTask = async ({ taskName, config, isRemote }) => {
  try {
    await invoke("update_priority_task", {
      taskName,
      config: JSON.stringify(config),
      isRemote: isRemote,
    });
    message.success("Priority任务更新成功");
    showUpdateModal.value = false;
    fetchTasks();
  } catch (err) {
    console.error("更新 Priority 任务失败", err);
    message.error("Priority任务更新失败");
  }
};

const route = useRoute(); // ✅
const router = useRouter();

const generateList = async (id) => {
  const stringId = typeof id === "object" && "$oid" in id ? id.$oid : id;

  console.log("准备传给 Rust 的 id 是：", stringId);

  await invoke("generate_list", { id: stringId });
  fetchTasks();
};

const syncRemoteTask = async (id) => {
  try {
    console.log("sync_remote_task的值:", id);
    await invoke("sync_remote_task", { alphaMissionList: id });

    // ✅ 任务同步成功后，设置该任务为远程运行
    const targetTask = taskStore.tasks.value.find((t) => t.name === id);
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
    const stringId = typeof id === "object" && "$oid" in id ? id.$oid : id;

    await invoke("update_remote_status", {
      id: stringId,
      updates: {
        is_remote: isRemote,
      },
    });

    message.success(`运行方式已更新为 ${isRemote ? "远程" : "本地"}`);
  } catch (err) {
    console.error("更新 isRemote 失败", err);
    message.error("运行方式更新失败");
  }
};

const pauseTask = async (task) => {
  console.log("Pause which task?", task.name);

  await invoke("pause_task", { taskName: task.name, isRemote: task.isRemote });
  fetchTasks();
};

const viewTask = (id) => {
  router.push(`/task/${id}`);
};

const getIsSuper = (task) => {
  return task && task.taskType === "super"; // task 不存在时返回 false
};

const getIsPriority = (task) => {
  return task && task.taskType === "priority"; // task 不存在时返回 false
};

const handleAddTask = async (task) => {
  try {
    const createdTask = await invoke("create_task", {
      newTask: {
        name: task.name,
        template: task.template,
        templatefile: task.templatefile,
        status: task.status ?? "waiting",
      },
    });

    // createdTask._id 是 Mongo 生成的 ObjectId
    taskStore.addTask({
      ...createdTask,
      _id: { $oid: createdTask._id }, // 保持和 Mongo 查询结果一致的结构
    });
    await fetchTasks();
    message.success("任务创建成功");
  } catch (err) {
    console.error("添加任务失败：", err);
    message.error("任务创建失败");
  }
};

const handleAddSuperTask = async (task) => {
  try {
    const createdTask = await invoke("create_task", {
      newTask: {
        name: task.name,
        template: task.template,
        templatefile: task.templatefile,
        status: task.status ?? "waiting",
        task_type: "super",
      },
    });

    taskStore.addTask({
      ...createdTask,
      _id: { $oid: createdTask._id }, // 保持和 Mongo 查询结果一致的结构
    });

    message.success("Super任务创建成功");
    await fetchTasks();
  } catch (err) {
    console.error("添加Super任务失败：", err);
    message.error("Super任务创建失败");
  }
};

const handleAddPriorityTask = async (task) => {
  try {
    const createdTask = await invoke("create_task", {
      newTask: {
        name: task.name,
        template: task.template,
        templatefile: task.templatefile,
        status: task.status ?? "waiting",
        task_type: "priority", // ✅ 区分 Priority
      },
    });

    taskStore.addTask({
      ...createdTask,
      _id: { $oid: createdTask._id },
    });

    message.success("Priority任务创建成功");
    await fetchTasks();
  } catch (err) {
    console.error("添加Priority任务失败：", err);
    message.error("Priority任务创建失败");
  }
};

const openStartModal = (task) => {
  // console.log(task); // 确保这里的 task 中包含 taskType

  selectedTask.value = task; // 直接修改 ref 的值
  showStartModal.value = true;
};

function openUpdateModal(task) {
  selectedTaskForUpdate.value = task;
  isSuperForUpdate.value = getIsSuper(task);
  isPriorityForUpdate.value = getIsPriority(task);
  showUpdateModal.value = true;
}

const handleStartTask = async ({ taskName, config, isRemote }) => {
  try {
    await invoke("start_task", {
      taskName,
      config: JSON.stringify(config), // 👈 关键：转为字符串
      isRemote: isRemote,
    });
    message.success("任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动任务失败", err);
    message.error("任务启动失败");
  }
};

const checkTaskStatus = async (task) => {
  try {
    const status = await invoke("check_task_status", {
      taskName: task.name, // 前端 task 对象的名字
      isRemote: task.isRemote, // 前端 task 对象的远程状态
    });
    taskRuntimeStatus.value[task.name] = status.trim();
  } catch (err) {
    console.error(`检查任务 ${task.name} 状态失败`, err);
    taskRuntimeStatus.value[task.name] = "error";
  }
};

const handlePriorityStartTask = async ({ taskName, config, isRemote }) => {
  try {
    await invoke("start_priority_task", {
      taskName,
      config: JSON.stringify(config), // 👈 关键：转为字符串
      isRemote: isRemote,
    });
    message.success("任务启动成功");
    fetchTasks();
  } catch (err) {
    console.error("启动任务失败", err);
    message.error("任务启动失败");
  }
};

import { useDialog } from "naive-ui";
const dialog = useDialog();

const deleteTask = (id) => {
  dialog.warning({
    title: "确认删除",
    content: "删除任务后将无法恢复，确定要继续吗？",
    positiveText: "确认",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        const stringId = typeof id === "object" && "$oid" in id ? id.$oid : id;

        console.log("准备传给 Rust 的 id 是：", stringId);

        await invoke("delete_task", { id: stringId });

        await fetchTasks();
      } catch (e) {
        console.error("删除任务失败", e);
      }
    },
  });
};

// // 使用 ref 来存储 unlisten
const unlisten = ref(null);

onMounted(async () => {
  // // 存储任务进度信息
  await fetchTasks(); // 组件加载时获取任务
  console.log("组件已挂载");

  taskStore.initializeProgressForTasks(tasks.value);
  console.log("初始化任务进度");

  // 监听任务进度更新事件
  const dispose = await listen("task-progress-update", async (event) => {
    const payload = event?.payload;

    handleTaskProgressUpdate(payload);

    // 检查 payload 格式
    if (
      payload &&
      typeof payload === "object" &&
      "collection" in payload &&
      "success" in payload &&
      "total" in payload
    ) {
      const { collection, success, total } = payload;

      // 检查和更新任务进度
      const priority_success =
        typeof payload.priority_success === "number" ? payload.priority_success : 0;
      const priority_total =
        typeof payload.priority_total === "number" ? payload.priority_total : 0;
      const is_remote =
        typeof payload.is_remote === "boolean" ? payload.is_remote : false;

      taskProgressMap.value[collection] = {
        success,
        total,
        priority_success,
        priority_total,
        is_remote,
      };

      // 如果任务完成，等待 5 分钟后自动暂停
      if (success >= total && total > 0) {
        console.log("taskStore:", taskStore);

        if (!Array.isArray(taskStore.tasks?.value)) {
          console.warn("taskStore.tasks.value 无效，跳过自动暂停逻辑");
          return;
        }

        const finishedTask = taskStore.tasks.value.find((t) => t.name === collection);
        if (finishedTask && finishedTask.status === "running") {
          console.log(`任务 ${collection} 已完成，5 分钟后自动暂停`);

          // ⏳ 等待 5 分钟再暂停
          setTimeout(async () => {
            const latestTask = taskStore.tasks.value.find((t) => t.name === collection);
            if (latestTask && latestTask.status === "running") {
              await pauseTask(latestTask);
              console.log(`任务 ${collection} 已在 5 分钟后自动暂停`);
            }
          }, 5 * 60 * 1000);
        }
      }
    } else {
      console.warn("收到格式不正确的 task-progress-update payload:", payload);
    }
  });

  // 显式调用后端的 frontend_ready 来通知后端发送进度更新
  await invoke("frontend_ready"); // 只有在前端准备好后调用

  unlisten.value = dispose;
});

// ✅ 切换回来时重新加载
watch(
  () => route.fullPath,
  () => {
    fetchTasks();
  }
);

onUnmounted(() => {
  // 在组件卸载时清理监听
  if (unlisten.value) {
    unlisten.value();
  }
});

const getStatusType = (status) =>
  status === "done" ? "success" : status === "running" ? "warning" : "default";

const getProgressBarStyle = (task) => {
  const progress = taskProgressMap.value[task.name] || {};
  const success = typeof progress.success === "number" ? progress.success : 0;
  const total =
    typeof progress.total === "number" && progress.total > 0 ? progress.total : 1;
  const priority_success =
    typeof progress.priority_success === "number" ? progress.priority_success : 0;

  const nonPrioritySuccess = Math.max(success - priority_success, 0);

  const greenPercent = (nonPrioritySuccess / total) * 100;
  const redPercent = (priority_success / total) * 100;

  const isSuper = task.taskType === "super";
  const mainColor = isSuper ? "#64b5f6" : "#a1e3a1";
  const priorityColor = "#caa969";
  const bgColor = "#f0f0f0";

  const layers = [];
  const sizes = [];
  const positions = [];

  if (total > 1) {
    layers.push(`linear-gradient(to right, ${mainColor}, ${mainColor})`);
    sizes.push(`${greenPercent}% 100%`);
    positions.push(`left top`);
  }

  if (total > 1) {
    layers.push(`linear-gradient(to right, ${priorityColor}, ${priorityColor})`);
    sizes.push(`${redPercent}% 100%`);
    positions.push(`${greenPercent}% 0`);
  }

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
};

// 安全地取嵌套字段
function getNested(obj, pathArray) {
  return pathArray.reduce(
    (acc, key) => (acc && acc[key] != null ? acc[key] : undefined),
    obj
  );
}

const getMaxConcurrent = (task) => {
  // 假定配置在 task.mission_config 或 task.config，根据你实际结构调整
  return (
    getNested(task, ["mission_config", "max_concurrent"]) ??
    getNested(task, ["config", "max_concurrent"]) ??
    "-"
  );
};

const getMaxMultiSimulationChildren = (task) => {
  return (
    getNested(task, ["mission_config", "max_multi_simulation_children"]) ??
    getNested(task, ["config", "max_multi_simulation_children"]) ??
    "-"
  );
};

const getPriorityConfig = (task) => {
  // 直接取 mission_config
  const cfg = getNested(task, ["mission_config"]) ?? getNested(task, ["config"]);
  if (!cfg) return null;

  // 输出调试
  console.log("priority config:", cfg);

  return cfg;
};

const hasConcurrencyConfig = (task) => {
  const mc =
    getNested(task, ["mission_config", "max_concurrent"]) ??
    getNested(task, ["config", "max_concurrent"]);
  const mm =
    getNested(task, ["mission_config", "max_multi_simulation_children"]) ??
    getNested(task, ["config", "max_multi_simulation_children"]);
  return task.status === "running" && (mc != null || mm != null);
};

// 定时检查 Priority 任务
const pollTaskStatus = () => {
  priorityTasks.value.forEach((task) => {
    if (task.status === "running") {
      checkTaskStatus(task);
    }
  });
};

// 每隔 600 秒刷新一次 (10 分钟)
setInterval(pollTaskStatus, 600000);

// 初始化时也检查一次
onMounted(() => {
  pollTaskStatus();
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

.task-card.priority-task {
  border: 2px solid #ff9800; /* 橙色边框 */
  box-shadow: 0 4px 12px rgba(255, 152, 0, 0.3); /* 橙色阴影 */
  background-color: #fffaf2; /* 浅橙背景 */
}

.task-card.priority-task .card-header {
  height: auto; /* 不固定高度 */
  min-height: auto; /* 移除 min-height 限制 */
  padding: 0 0; /* 左右 padding 保留 */
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
}

.card-header {
  position: relative;
  width: 100%; /* 确保宽度是100% */
  height: 40px; /* 固定高度 */
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
  height: 100%; /* 高度100%填充 */
  width: 100%; /* 宽度100%填充 */
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
  width: 100%; /* 确保内容占满整个区域 */
  padding: 0 12px;
  font-weight: bold;
  font-size: 14px;
}

.task-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
