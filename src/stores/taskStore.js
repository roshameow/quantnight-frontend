// src/stores/taskStore.js
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

const emptyProgress = (task, previous) => ({
  success: 0, error: 0, total: 0,
  priority_success: 0, priority_error: 0, priority_total: 0,
  is_remote: task.isRemote === true,
  stale: true,
  lastError: null,
  // Keep the ordering watermark when switching sources, not the old counts.
  updated_at: previous?.updated_at,
})

const validCount = (value) => typeof value === 'number' && Number.isFinite(value) && value >= 0
const validVersion = (value) => Number.isSafeInteger(value) && value >= 0
const taskId = (task) => typeof task?._id === 'object' ? task._id?.$oid : task?._id

export const useTaskStore = defineStore('task', () => {
  // One assignment publishes both the task list and its reconciled progress cache.
  const state = ref({ tasks: [], progress: {} })
  const tasks = computed(() => state.value.tasks)
  const taskProgressMap = computed(() => state.value.progress)

  const replaceTasks = (tasksList) => {
    const previousTasks = new Map(tasks.value.map((task) => [task.name, task]))
    const progress = Object.fromEntries(tasksList.map((task) => {
      const old = state.value.progress[task.name]
      const sameIdentity = taskId(previousTasks.get(task.name)) === taskId(task)
      return [task.name, old && sameIdentity && old.is_remote === (task.isRemote === true)
        ? old : emptyProgress(task, old)]
    }))
    state.value = { tasks: [...tasksList], progress }
  }

  const addTask = (task) => replaceTasks([...tasks.value, task])
  const clearTasks = () => replaceTasks([])
  const removeTask = (name) => replaceTasks(tasks.value.filter((task) => task.name !== name))
  const setTaskSource = (name, isRemote) => replaceTasks(tasks.value.map((task) =>
    task.name === name ? { ...task, isRemote } : task))

  const acceptEvent = (event, isError) => {
    if (!event || typeof event !== 'object' || typeof event.collection !== 'string') return null
    const task = tasks.value.find((task) => task.name === event.collection)
    if (!task) return null
    // Legacy payloads omit identity. null is only valid for metadata failures.
    if (event.task_id !== undefined && !(isError && event.task_id === null) &&
        (typeof event.task_id !== 'string' || event.task_id !== taskId(task))) return null
    // null is reserved for failures where the backend could not resolve a source.
    if (!(isError && event.is_remote === null) &&
        event.is_remote !== (task.isRemote === true)) return null
    const previous = taskProgressMap.value[task.name]
    const timestamp = event.updated_at
    if (isError && timestamp === undefined) return null
    if (timestamp !== undefined && !validVersion(timestamp)) return null
    // Legacy events are accepted until this task has received a versioned event.
    if (previous?.updated_at !== undefined &&
        (timestamp === undefined || timestamp < previous.updated_at)) return null
    return { task, previous, timestamp }
  }

  const updateProgress = (event) => {
    const accepted = acceptEvent(event, false)
    if (!accepted || !validCount(event.success) || !validCount(event.total)) return false
    const counts = {
      success: event.success, total: event.total, error: event.error ?? 0,
      priority_success: event.priority_success ?? 0,
      priority_error: event.priority_error ?? 0,
      priority_total: event.priority_total ?? 0,
    }
    if (!Object.values(counts).every(validCount)) return false
    taskProgressMap.value[event.collection] = {
      ...counts,
      is_remote: accepted.task.isRemote === true,
      updated_at: accepted.timestamp,
      stale: false,
      lastError: null,
    }
    return true
  }

  const updateProgressError = (event) => {
    const accepted = acceptEvent(event, true)
    if (!accepted || typeof event.error !== 'string') return false
    taskProgressMap.value[event.collection] = {
      ...(accepted.previous ?? emptyProgress(accepted.task)),
      updated_at: accepted.timestamp,
      stale: true,
      lastError: event.error,
    }
    return true
  }

  // Discovery failures have no collection: stale only the affected current tasks.
  const markProgressStale = (event) => {
    if (!event || !['all', 'remote'].includes(event.scope) ||
        typeof event.error !== 'string' || !validVersion(event.updated_at)) return false
    const progress = { ...taskProgressMap.value }
    let changed = false
    for (const task of tasks.value) {
      if (event.scope === 'remote' && task.isRemote !== true) continue
      const old = progress[task.name]
      if (old?.updated_at !== undefined && event.updated_at < old.updated_at) continue
      progress[task.name] = {
        ...(old ?? emptyProgress(task)),
        stale: true,
        lastError: event.error,
        updated_at: event.updated_at,
      }
      changed = true
    }
    if (changed) state.value = { tasks: tasks.value, progress }
    return changed
  }

  // Retained for existing callers; source always comes from the current task.
  const initializeProgressForTasks = () => replaceTasks(tasks.value)

  return {
    // Register the backing state with Pinia (subscriptions, devtools, hydration).
    state,
    tasks, taskProgressMap, replaceTasks, addTask, clearTasks, removeTask,
    setTaskSource, updateProgress, updateProgressError, markProgressStale, initializeProgressForTasks,
  }
})
