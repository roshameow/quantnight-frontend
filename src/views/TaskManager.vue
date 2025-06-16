<template>
  <div style="padding: 0px">
    <n-page-header title="回测任务管理" style="margin-bottom: 20px">
      <template #extra>
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
            margin-left: 20px;
          "
        >
          ➕ 添加Super任务
        </n-button>
      </template>
    </n-page-header>

    <n-empty v-if="taskIsEmpty" description="暂无任务" />

    <n-grid
      x-gap="20"
      y-gap="20"
      responsive="screen"
      style="display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr))"
    >
      <n-gi
        v-for="task in tasks"
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
              <!-- 进度条背景 -->
              <div class="progress-background" :style="getProgressBarStyle(task)" />

              <!-- 标题 + 状态 -->
              <div class="header-content">
                <span class="task-name">{{ task.name }}</span>
                <n-tag :type="getStatusType(task.status)" size="small" round>
                  {{ task.status }}
                </n-tag>
              </div>
            </div>
          </template>
          <div>
            任务进度：
            {{ taskProgressMap[task.name]?.success ?? 0 }}
            /
            {{ taskProgressMap[task.name]?.total ?? 0 }}
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
                <template #icon>📄</template>查看
              </n-button>
              <n-button
                size="small"
                @click="generateList(task._id)"
                :disabled="task.status === 'running'"
              >
                <template #icon>🧾</template>生成list
              </n-button>
              <n-button
                size="small"
                @click="syncRemoteTask(task.name)"
                :disabled="task.status === 'running'"
              >
                <template #icon>☁️</template>发送到远程
              </n-button>
              <n-button size="small" @click="openStartModal(task)">
                <template #icon>🚀</template>启动
              </n-button>
              <n-button size="small" @click="pauseTask(task)">
                <template #icon>⏸️</template>暂停
              </n-button>
              <n-button size="small" type="error" @click="deleteTask(task._id)">
                <template #icon>🗑️</template>删除
              </n-button>
            </n-space>
          </n-space>
        </n-card>
      </n-gi>
    </n-grid>

    <AddTaskModal v-model:show="showAddModal" @add-task="handleAddTask" />
    <AddSuperTaskModal
      v-model:show="showAddSuperModal"
      @add-super-task="handleAddSuperTask"
    />

    <StartTaskModal
      v-model:show="showStartModal"
      :task="selectedTask"
      :isSuper="getIsSuper(selectedTask)"
      @start-task="handleStartTask"
      @start-super-task="handleSuperStartTask"
    />
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
console.log("invoke 是", invoke); // 应该是一个 function，而不是 undefined
import { useRoute, useRouter } from "vue-router"; // ✅ 确保 useRoute 导入
import { useTaskStore } from "../stores/taskStore";
import AddTaskModal from "../views/AddTaskModal.vue";
import AddSuperTaskModal from "../views/AddSuperTaskModal.vue";
import StartTaskModal from "../views/StartTaskModal.vue";

import { useMessage } from "naive-ui";
import { computed } from "vue";

const tasks = computed(() => taskStore.tasks.value);
const message = useMessage(); // 在 setup() 中获取消息方法

const showAddModal = ref(false);
const showAddSuperModal = ref(false);
const showStartModal = ref(false);
const selectedTask = ref(null); // 默认 null，也可以初始化成一个空对象

const route = useRoute(); // ✅
const router = useRouter();
const taskStore = useTaskStore();

const taskIsEmpty = computed(() => {
  return !taskStore.tasks?.value || taskStore.tasks.value.length === 0;
});

// ✅ 注意：.value 是必须的
const fetchTasks = async () => {
  try {
    const result = await invoke("get_all_tasks");
    // taskStore.tasks.value = result;
    console.log("get_all_tasks", result);

    const activeTasks = result.filter((task) => task.status !== "deactive");
    taskStore.tasks.value = [...activeTasks]; // 强制重新赋值，确保模板更新
    console.log("taskStore.tasks.value", taskStore.tasks.value);
    console.log("taskStore.tasks.length", taskStore.tasks.value.length);
  } catch (e) {
    console.error("加载任务失败", e);
  }
};

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

    await invoke("update_task", {
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

    message.success("任务创建成功");
    await fetchTasks();
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

const openStartModal = (task) => {
  console.log(task); // 确保这里的 task 中包含 taskType

  selectedTask.value = task; // 直接修改 ref 的值
  showStartModal.value = true;
};

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

const handleSuperStartTask = async ({ taskName, config, isRemote }) => {
  try {
    await invoke("start_super_task", {
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

// // 存储任务进度信息
const taskProgressMap = ref({});

// // 使用 ref 来存储 unlisten
const unlisten = ref(null);

onMounted(async () => {
  console.log("组件已挂载");
  const dispose = await listen("task-progress-update", async (event) => {
    const payload = event?.payload;

    if (
      payload &&
      typeof payload === "object" &&
      "collection" in payload &&
      "success" in payload &&
      "total" in payload
    ) {
      const { collection, success, total } = payload;
      taskProgressMap.value[collection] = { success, total };

      // ✅ 检查任务是否完成
      if (success >= total && total > 0) {
        const finishedTask = taskStore.tasks.value.find((t) => t.name === collection);
        if (finishedTask && finishedTask.status === "running") {
          console.log(`任务 ${collection} 已完成，5 分钟后自动暂停`);

          // ⏳ 等待 5 分钟再暂停
          setTimeout(async () => {
            // 再次检查任务状态，确保还在运行
            const latestTask = taskStore.tasks.value.find((t) => t.name === collection);
            if (latestTask && latestTask.status === "running") {
              await pauseTask(latestTask); // 自动暂停任务
              console.log(`任务 ${collection} 已在 5 分钟后自动暂停`);
            }
          }, 5 * 60 * 1000); // 5 分钟 = 300000 毫秒
        }
      }
    } else {
      console.warn("收到格式不正确的 task-progress-update payload:", payload);
    }
  });

  unlisten.value = dispose;

  // 加载已有任务数据
  await fetchTasks();

  // ✅ 告诉后端前端准备好监听了
  await invoke("frontend_ready"); // 后端根据这个再发送一次进度也可以
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
  const progress = taskProgressMap.value[task.name];
  const percent =
    progress && progress.total > 0
      ? Math.min((progress.success / progress.total) * 100, 100)
      : 0;

  const isSuper = task.taskType === "super";

  return {
    width: "100%",
    height: "100%",
    position: "absolute",
    top: 0,
    left: 0,
    zIndex: 0,
    borderRadius: "10px 10px 0 0",
    background: isSuper
      ? `linear-gradient(to right, #64b5f6 ${percent}%, #f0f0f0 ${percent}%)`
      : `linear-gradient(to right, #a1e3a1 ${percent}%, #f0f0f0 ${percent}%)`,
  };
};
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
