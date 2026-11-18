// src/stores/taskStore.js
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { newTaskRequestId, failureKind, errorStage, validOperationStage, taskLaunchMatches, taskLaunchValue } from '../utils/taskDiagnostics.js'

const emptyProgress = (task, previous) => ({
  success: 0, error: 0, cancelled: 0, total: 0,
  priority_success: 0, priority_error: 0, priority_cancelled: 0, priority_total: 0,
  is_remote: task.isRemote === true,
  stale: true,
  lastError: null,
  // Keep the ordering watermark when switching sources, not the old counts.
  updated_at: previous?.updated_at,
})

const validCount = (value) => typeof value === 'number' && Number.isFinite(value) && value >= 0
const validVersion = (value) => Number.isSafeInteger(value) && value >= 0
const taskId = (task) => typeof task?._id === 'object' ? task._id?.$oid : task?._id

// Scope replies and local action diagnostics to their original task/host.
const sameStopTarget = (task, target) => !!task && !!target &&
  task.name === target.name && typeof taskId(task) === 'string' && !!taskId(task) &&
  taskId(task) === taskId(target) && typeof target.isRemote === 'boolean' &&
  (task.isRemote === true) === target.isRemote && taskLaunchMatches(task, target) &&
  (target.expectedTaskType === undefined || (task.taskType ?? 'regular') === target.expectedTaskType) &&
  (target.expectedAuthProfile === undefined || (task.auth_profile ?? 'user1') === target.expectedAuthProfile)

// The native adapter owns stopping and receipt verification. Historical proof
// fields and confirmation-basis metadata are not another GUI success gate.
export function validStopReceipt(target, receipt) {
  return receipt?.confirmed === true && typeof receipt.taskId === 'string' &&
    typeof receipt.taskName === 'string' &&
    sameStopTarget(target, { _id: receipt.taskId, name: receipt.taskName, isRemote: receipt.isRemote })
}

const isRelaunchedAfterPause = (task, operation) => (
  operation?.status === 'paused' &&
  operation.pausedLaunchAttemptId !== undefined &&
  taskLaunchValue(task) !== operation.pausedLaunchAttemptId
)

const isRunningOrRelaunchedAfterPause = (task, operation) => (
  operation?.status === 'paused' && (
    ['running', 'launching'].includes(task?.status) ||
    isRelaunchedAfterPause(task, operation)
  )
)

export function taskStopDisplayStatus(task, operation) {
  if (sameStopTarget(task, operation)) {
    if (operation.status === 'stopping') return 'stopping'
    if (operation.status === 'stop_unconfirmed') {
      if (operation.failureSource === 'client_disposed') return '停止回执核验中断'
      if (operation.failureSource === 'receipt_validation') return '停止回执校验失败'
      return operation.failureKind === 'timeout' ? '停止确认超时' : '停止执行失败'
    }
    if (operation.status === 'paused' && !isRunningOrRelaunchedAfterPause(task, operation) &&
        validStopReceipt(task, operation.receipt)) return 'paused'
  }
  // List state is a snapshot, not validation of the last pause request.
  return task?.status === 'paused' ? 'paused' : null
}

export const useTaskStore = defineStore('task', () => {
  // One assignment publishes both the task list and its reconciled progress cache.
  const state = ref({ tasks: [], progress: {}, stopOperations: {} })
  const tasks = computed(() => state.value.tasks)
  const taskProgressMap = computed(() => state.value.progress)
  const taskStopOperationMap = computed(() => state.value.stopOperations)
  let stopGeneration = 0

  const replaceTasks = (tasksList) => {
    const previousTasks = new Map(tasks.value.map((task) => [task.name, task]))
    const progress = Object.fromEntries(tasksList.map((task) => {
      const old = state.value.progress[task.name]
      const sameIdentity = taskId(previousTasks.get(task.name)) === taskId(task)
      return [task.name, old && sameIdentity && old.is_remote === (task.isRemote === true)
        ? old : emptyProgress(task, old)]
    }))
    const stopOperations = Object.fromEntries(tasksList.flatMap((task) => {
      const operation = taskStopOperationMap.value[task.name]
      if (!sameStopTarget(task, operation)) return []
      if (isRunningOrRelaunchedAfterPause(task, operation)) return []
      if (taskLaunchValue(task) !== operation.observedLaunchAttemptId) return []
      return [[task.name, operation]]
    }))
    state.value = { tasks: [...tasksList], progress, stopOperations }
  }

  const addTask = (task) => replaceTasks([...tasks.value, task])
  const clearTasks = () => replaceTasks([])
  const removeTask = (name) => replaceTasks(tasks.value.filter((task) => task.name !== name))
  const setTaskSource = (name, isRemote) => replaceTasks(tasks.value.map((task) =>
    task.name === name ? { ...task, isRemote } : task))

  const currentStopTask = (target) => tasks.value.find((task) => sameStopTarget(task, target))
  const beginTaskStop = (target) => {
    const current = currentStopTask(target)
    if (!current || taskStopOperationMap.value[target.name]?.status === 'stopping') return null
    const operation = { _id: taskId(target), name: target.name, isRemote: target.isRemote,
      ...(target.expectedLaunchAttemptId != null ? { expectedLaunchAttemptId: target.expectedLaunchAttemptId } : {}),
      ...(target.expectedLaunchAbsent === true ? { expectedLaunchAbsent: true } : {}),
      ...(target.expectedTaskType !== undefined ? { expectedTaskType: target.expectedTaskType, taskType: target.taskType } : {}),
      ...(target.expectedAuthProfile !== undefined ? { expectedAuthProfile: target.expectedAuthProfile, auth_profile: target.auth_profile } : {}),
      ...(Object.hasOwn(target, 'launchAttemptId') ? { launchAttemptId: target.launchAttemptId } : {}),
      status: 'stopping', generation: ++stopGeneration, observedLaunchAttemptId: taskLaunchValue(current),
      requestId: newTaskRequestId('stop'), stage: 'native_request',
      detail: '已发起原生停止调用，等待阶段回报和最终停止凭据', error: null }
    state.value = { ...state.value,
      stopOperations: { ...taskStopOperationMap.value, [target.name]: operation } }
    return operation
  }
  const ownsStop = (target, operation) => {
    const current = currentStopTask(target)
    return current && operation && sameStopTarget(target, operation) &&
      taskStopOperationMap.value[target.name]?.generation === operation.generation &&
      taskLaunchValue(current) === operation.observedLaunchAttemptId
  }
  const updateTaskStopStage = (target, operation, event) => {
    if (!ownsStop(target, operation) || !validOperationStage(event) || event.operation !== 'stop' ||
        event.taskId !== taskId(target) || event.name !== target.name || event.isRemote !== target.isRemote ||
        event.requestId !== operation.requestId) return false
    const current = taskStopOperationMap.value[target.name]
    if (current.status !== 'stopping' || current.requestId !== event.requestId) return false
    state.value = { ...state.value, stopOperations: { ...taskStopOperationMap.value,
      [target.name]: { ...current, stage: event.stage, detail: event.message } } }
    return true
  }
  const markTaskStopUnconfirmed = (target, operation, error, failureSource = 'native') => {
    if (!ownsStop(target, operation) || taskStopOperationMap.value[target.name]?.status !== 'stopping') return false
    const current = taskStopOperationMap.value[target.name]
    const failedStage = errorStage(error)
    state.value = { ...state.value, stopOperations: { ...taskStopOperationMap.value,
      [target.name]: { ...taskStopOperationMap.value[target.name], status: 'stop_unconfirmed',
        stage: failedStage ?? current.stage,
        detail: failedStage && failedStage !== current.stage ? '原生桥在该阶段返回错误（完整原因见下文）' : current.detail,
        failureKind: failureKind(error), failureSource, error: String(error) } } }
    return true
  }
  // Only a matched native outcome can enter this action commit.
  const confirmTaskPaused = (target, receipt, operation) => {
    if (!ownsStop(target, operation) || taskStopOperationMap.value[target.name]?.status !== 'stopping' ||
        !validStopReceipt(target, receipt)) return false
    const current = currentStopTask(target)
    state.value = { ...state.value,
      tasks: tasks.value.map((task) => task === current ? { ...task, status: 'paused',
        stopConfirmedAt: receipt.stopConfirmedAt, stopConfirmedRemote: receipt.isRemote,
        stopConfirmedTaskId: receipt.taskId, stopConfirmationBasis: receipt.confirmationBasis } : task),
      stopOperations: { ...taskStopOperationMap.value,
        [target.name]: { ...taskStopOperationMap.value[target.name], status: 'paused', pausedLaunchAttemptId: taskLaunchValue(current), receipt: { ...receipt } } } }
    return true
  }
  // Explicit successful start supersedes a locally displayed pause, not real counts.
  const clearTaskStop = (target) => {
    if (!currentStopTask(target)) return false
    const stopOperations = { ...taskStopOperationMap.value }
    delete stopOperations[target.name]
    const nextTasks = tasks.value.map((task) => {
      if (!sameStopTarget(task, target)) return task
      const { stopConfirmedAt, stopConfirmedRemote, stopConfirmedTaskId, stopConfirmationBasis, ...rest } = task
      return rest.status === 'paused' ? { ...rest, status: 'running' } : rest
    })
    state.value = { ...state.value, tasks: nextTasks, stopOperations }
    return true
  }

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

  // Optional campaign metadata shares the existing task/source/version guards.
  // Only flat, validated item counters may populate its actual-count snapshot;
  // registry reserved/status fields are never substituted for real counts.
  const campaignMetadata = (event, accepted, counts) => {
    const payload = event.priority_campaign
    const previous = accepted.previous?.priority_campaign
    if (payload === undefined) return counts ? undefined : previous
    if (!payload || typeof payload !== 'object' || Array.isArray(payload) ||
        payload.task_id !== taskId(accepted.task) ||
        payload.collection !== accepted.task.name ||
        payload.is_remote !== (accepted.task.isRemote === true) ||
        payload.updated_at !== accepted.timestamp) throw new Error('invalid campaign metadata')
    const sameIdentity = previous?.task_id === payload.task_id && previous?.is_remote === payload.is_remote
    const incoming = sameIdentity && payload.reason !== 'root_pool_not_registered' &&
      payload.last_error && !payload.campaign_id && !payload.pool_id
      ? { ...payload, campaign_id: previous.campaign_id, pool_id: previous.pool_id,
          mode: previous.mode, stage_distribution: previous.stage_distribution, root_count: previous.root_count,
          waiting_feedback: previous.waiting_feedback, waiting_slots: previous.waiting_slots,
          completed: previous.completed, blocked: previous.blocked, source_tasks: previous.source_tasks,
          source_summaries: previous.source_summaries,
          stage: previous.stage, status: previous.status,
          reason: previous.reason, current_round: previous.current_round, max_rounds: previous.max_rounds,
          max_per_round: previous.max_per_round, unresolved: previous.unresolved }
      : { ...payload }
    const sameCampaign = sameIdentity && previous?.pool_id === incoming.pool_id &&
      previous?.campaign_id === incoming.campaign_id && previous?.mode === incoming.mode
    incoming.counts = counts && incoming.stats_confirmed === true
      ? { ...counts } : (sameCampaign ? previous?.counts ?? null : null)
    return incoming
  }

  const updateProgress = (event) => {
    const accepted = acceptEvent(event, false)
    if (!accepted || !validCount(event.success) || !validCount(event.total)) return false
    const counts = {
      success: event.success, total: event.total, error: event.error ?? 0,
      // Additive read projection; older native events omit cancellation counts.
      cancelled: event.cancelled ?? 0,
      priority_cancelled: event.priority_cancelled ?? 0,
      priority_success: event.priority_success ?? 0,
      priority_error: event.priority_error ?? 0,
      priority_total: event.priority_total ?? 0,
    }
    if (!Object.values(counts).every(validCount)) return false
    let priorityCampaign
    try { priorityCampaign = campaignMetadata(event, accepted, counts) } catch { return false }
    taskProgressMap.value[event.collection] = {
      ...(priorityCampaign ? { priority_campaign: priorityCampaign } : {}),
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
    let priorityCampaign
    try { priorityCampaign = campaignMetadata(event, accepted) } catch { return false }
    taskProgressMap.value[event.collection] = {
      ...(accepted.previous ?? emptyProgress(accepted.task)),
      ...(priorityCampaign ? { priority_campaign: priorityCampaign } : {}),
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
    if (changed) state.value = { ...state.value, progress }
    return changed
  }

  // Retained for existing callers; source always comes from the current task.
  const initializeProgressForTasks = () => replaceTasks(tasks.value)

  return {
    // Register the backing state with Pinia (subscriptions, devtools, hydration).
    state,
    tasks, taskProgressMap, taskStopOperationMap, replaceTasks, addTask, clearTasks, removeTask,
    setTaskSource, beginTaskStop, updateTaskStopStage, markTaskStopUnconfirmed, confirmTaskPaused, clearTaskStop,
    updateProgress, updateProgressError, markProgressStale, initializeProgressForTasks,
  }
})
