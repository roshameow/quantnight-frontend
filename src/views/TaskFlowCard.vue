<template>
  <div class="task-flow-card">
    <h3>{{ flow.name }}</h3>
    <div class="task-timeline">
      <div
        v-for="task in flow.tasks"
        :key="task.id"
        class="task-node"
        :class="task.status"
      >
        <div class="task-name">{{ task.name }}</div>
        <div class="task-status">{{ task.status }}</div>
        <n-button size="tiny" @click="runTask(task)">运行</n-button>
      </div>
    </div>
  </div>
</template>

<script setup>
import { NButton } from 'naive-ui'
const props = defineProps({
  flow: Object
})

function runTask(task) {
  task.status = 'running'
  window.__TAURI__.invoke('run_script', { script: task.script }).then(() => {
    task.status = 'done'
  }).catch(() => {
    task.status = 'error'
  })
}
</script>

<style scoped>
.task-flow-card {
  border: 1px solid #ddd;
  padding: 16px;
  margin-bottom: 24px;
  border-radius: 12px;
  background: #fff;
  box-shadow: 0 0 6px rgba(0,0,0,0.06);
}
.task-timeline {
  display: flex;
  gap: 16px;
  margin-top: 12px;
}
.task-node {
  padding: 8px;
  min-width: 100px;
  border-radius: 8px;
  background: #f0f0f0;
  text-align: center;
  box-shadow: 0 1px 4px rgba(0,0,0,0.1);
}
.task-node.running {
  background: #e6f5ec;
  border-left: 4px solid #18a058;
}
.task-node.done {
  background: #eaf3fd;
  border-left: 4px solid #2080f0;
}
.task-node.error {
  background: #fdeaea;
  border-left: 4px solid red;
}
.task-name {
  font-weight: bold;
}
</style>
