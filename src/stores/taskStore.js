// src/stores/taskStore.js
import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useTaskStore = defineStore('task', () => {
  // 所有任务列表
  const tasks = ref([])

  /**
   * 添加一个新任务（不生成本地 id，保留 Mongo 的 _id）
   * @param {Object} task - 任务对象
   */
  const addTask = (task) => {
    tasks.value.unshift(task) // 新任务插入顶部
  }

  /**
   * 清空任务（可用于调试或重置）
   */
  const clearTasks = () => {
    tasks.value = []
  }

  return {
    tasks,
    addTask,
    clearTasks
  }
})

