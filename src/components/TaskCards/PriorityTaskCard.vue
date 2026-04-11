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
          <n-tag :type="getStatusType(task.status)" size="small" round>
            {{ task.status }}
          </n-tag>
        </div>
      </div>
    </template>

    <div v-if="progress" style="margin-bottom: 4px">
      <div style="font-size: 12px; margin-bottom: 2px">
        <n-text type="success" title="成功">
          ✓ {{ progress?.success ?? 0 }}
        </n-text>
        &nbsp;|&nbsp;
        <n-text type="error" title="失败">
          ✗ {{ progress?.error ?? 0 }}
        </n-text>
        &nbsp;|&nbsp;
        <n-text depth="3" title="总计">
          Σ {{ progress?.total ?? 0 }}
        </n-text>
      </div>
    </div>
    <div>
      任务状态：
      {{ taskRuntimeStatus ?? "未知" }}
    </div>
    <div
      v-if="task.status === 'running' && priorityConfig"
      style="margin-top: 4px; font-size: 12px; color: #555"
    >
      优先级: {{ priorityConfig.priority ?? "-" }} &nbsp;|&nbsp; Filter
      Params: {{ priorityConfig.filter_params ?? "-" }}
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

<script setup>
import { computed } from 'vue';

const props = defineProps({
  task: {
    type: Object,
    required: true,
  },
  taskRuntimeStatus: {
    type: String,
    default: "未知",
  },
  progress: {
    type: Object,
    default: () => ({}),
  },
});

defineEmits(['update:isRemote', 'start', 'update', 'pause', 'delete']);

const progressBarStyle = computed(() => {
  const p = props.progress || {};
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
  const cfg = getNested(props.task, ["mission_config"]) ?? getNested(props.task, ["config"]);
  return cfg;
});

const getStatusType = (status) =>
  status === "done" ? "success" : status === "running" ? "warning" : "default";
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
.task-card.priority-task {
  border: 2px solid #ff9800; /* 橙色边框 */
  box-shadow: 0 4px 12px rgba(255, 152, 0, 0.3); /* 橙色阴影 */
  background-color: #fffaf2; /* 浅橙背景 */
}
.card-header {
  position: relative;
  width: 100%;
  height: 40px;
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
}
.task-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
