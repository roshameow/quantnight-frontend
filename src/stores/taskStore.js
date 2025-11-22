// src/stores/taskStore.js
import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useTaskStore = defineStore('task', () => {
  const tasks = ref([])
  const taskProgressMap = ref({})

  // 添加新任务
  const addTask = (task) => {
    tasks.value.push(task)
  }

  // 清空任务
  const clearTasks = () => {
    tasks.value = []
  }

  // 更新任务的进度
  const updateProgress = (progress) => {
    // 保证更新触发响应式
    taskProgressMap.value[progress.collection] = {
      success: progress.success,
      total: progress.total,
      priority_success: progress.priority_success ?? 0,
      priority_total: progress.priority_total ?? 0,
      is_remote: progress.is_remote ?? false,
    }
  }

  // 初始化任务的进度
  const initializeProgressForTasks = (tasksList) => {
    tasksList.forEach((task) => {
      if (!taskProgressMap.value[task.name]) {
        taskProgressMap.value[task.name] = { success: 0, total: 0, priority_success: 0, priority_total: 0, is_remote: false }
        console.log("Progress initialized for task", task.name)
      }
    })
  }

  return {
    tasks,
    taskProgressMap,
    addTask,
    clearTasks,
    updateProgress,
    initializeProgressForTasks,
  }
})
