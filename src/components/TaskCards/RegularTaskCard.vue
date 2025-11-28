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
          <n-tag :type="getStatusType(task.status)" size="small" round>
            {{ task.status }}
          </n-tag>
        </div>
      </div>
    </template>
    <div v-if="progress">
      <div>
        任务进度：
        {{ (progress?.success ?? 0) - (progress?.priority_success ?? 0) }}+
        {{ progress?.priority_success ?? 0 }}
        /
        {{ (progress?.total ?? 0) - (progress?.priority_total ?? 0) }}+
        {{ progress?.priority_total ?? 0 }}
      </div>
    </div>
    <div v-if="task.status === 'running'">
      <template v-if="hasConcurrencyConfig">
        <div style="margin-top: 4px; font-size: 12px; color: #555">
          最大并发: {{ maxConcurrent }} &nbsp;|&nbsp; 子任务上限:
          {{ maxMultiSimulationChildren }}
        </div>
      </template>
    </div>
    <n-space style="margin-top: 2px">
      <n-switch
        :value="task.isRemote"
        @update:value="(val) => $emit('update:isRemote', val)"
        :checked-value="true"
        :unchecked-value="false"
      >
        <template #checked>远程</template>
        <template #unchecked>本地</template>
      </n-switch>
      <n-space wrap size="small" style="margin-top: 10px">
        <n-button size="small" @click="$emit('view')">
          <template #icon>[ ]</template>查看
        </n-button>
        <n-button
          size="small"
          @click="$emit('generateList')"
          :disabled="task.status === 'running'"
        >
          <template #icon>=</template>生成list
        </n-button>
        <n-button
          size="small"
          @click="$emit('syncRemote')"
          :disabled="task.status === 'running'"
        >
          <template #icon>-></template>发送到远程
        </n-button>
        <n-button size="small" @click="$emit('start')">
          <template #icon>>></template>启动
        </n-button>
        <n-button size="small" @click="$emit('pause')">
          <template #icon>| |</template>暂停
        </n-button>
        <n-button size="small" type="error" @click="$emit('delete')">
          <template #icon>x</template>删除
        </n-button>
      </n-space>
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
  progress: {
    type: Object,
    default: () => ({}),
  },
});

defineEmits(['update:isRemote', 'view', 'generateList', 'syncRemote', 'start', 'pause', 'delete']);

const getStatusType = (status) =>
  status === "done" ? "success" : status === "running" ? "warning" : "default";

const progressBarStyle = computed(() => {
  const p = props.progress || {};
  const success = typeof p.success === "number" ? p.success : 0;
  const total = typeof p.total === "number" && p.total > 0 ? p.total : 1;
  const priority_success = typeof p.priority_success === "number" ? p.priority_success : 0;

  const nonPrioritySuccess = Math.max(success - priority_success, 0);
  const greenPercent = (nonPrioritySuccess / total) * 100;
  const redPercent = (priority_success / total) * 100;

  const isSuper = props.task.taskType === "super";
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
}
.task-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
