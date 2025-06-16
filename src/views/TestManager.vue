<script setup>
import { ref } from 'vue'
import TaskFlowCard from '../views/TaskFlowCard.vue'
import AddFlowModal from '../views/AddFlowModal.vue' // 新增

const taskFlows = ref([]) // 初始为空，可由用户添加

const showAddModal = ref(false)
const scriptPool = [
  { name: '回测', script: 'a1.py' },
  { name: '拉取result', script: 'a2.py' },
  { name: '拉取pnl', script: 'a3.py' },
  { name: '聚类', script: 'a4.py' },
  { name: 'B1', script: 'b1.py' },
  { name: 'B2', script: 'b2.py' },
]

function addNewFlow(flowName, selectedTasks) {
  const newFlow = {
    id: `flow-${Date.now()}`,
    name: flowName,
    tasks: selectedTasks.map((t, i) => ({
      id: i + 1,
      name: t.name,
      script: t.script,
      status: 'pending'
    }))
  }
  taskFlows.value.push(newFlow)
}
</script>

<template>
  <div class="timeline-board">
    <n-button @click="showAddModal = true">➕ 添加任务流</n-button>
    <AddFlowModal
      v-if="showAddModal"
      :script-pool="scriptPool"
      @close="showAddModal = false"
      @submit="addNewFlow"
    />
    <TaskFlowCard
      v-for="flow in taskFlows"
      :key="flow.id"
      :flow="flow"
    />
  </div>
</template>
