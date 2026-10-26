// Latest-only task snapshots; progress refresh never blocks operation feedback.
export function createTaskFetcher({ load, replace, refresh, isActive, onError, onRefreshError }) {
  let latestRequest = 0
  const fetch = async () => {
    if (!isActive()) return false
    const request = ++latestRequest
    try {
      const result = await load()
      if (!isActive() || request !== latestRequest) return false
      replace(result.filter((task) => task.status !== 'deactive'))
      // Invoke immediately, but do not await background progress work.
      try { Promise.resolve(refresh()).catch(onRefreshError) } catch (error) { onRefreshError(error) }
      return true
    } catch (error) {
      if (isActive() && request === latestRequest) onError(error)
      return false
    }
  }
  fetch.invalidate = () => { latestRequest++ }
  return fetch
}

// Invalidate pre-mutation reads and reads started while the command was pending.
// The final invalidation and local commit have no await gap between them.
export async function mutateTaskSource({ fetch, update, commit = () => {} }) {
  fetch.invalidate()
  try {
    await update()
    fetch.invalidate()
    commit()
  } catch (error) {
    fetch.invalidate()
    throw error
  }
}

// All subscriptions must be installed before starting backend progress delivery.
export function createTaskProgressLifecycle({ listen, ready, fetch, poll, onProgress, onError, onRefreshError,
  setIntervalFn = setInterval, clearIntervalFn = clearInterval }) {
  let disposed = false
  const cleanups = new Set()
  let interval
  const stop = () => {
    disposed = true
    if (interval !== undefined) clearIntervalFn(interval)
    for (const cleanup of cleanups) cleanup()
    cleanups.clear()
  }
  const subscribe = async (name, handler) => {
    const cleanup = await listen(name, (event) => { if (!disposed) handler(event.payload) })
    if (disposed) cleanup()
    else cleanups.add(cleanup)
  }
  const start = async () => {
    try {
      await Promise.all([
        subscribe('task-progress-update', onProgress),
        subscribe('task-progress-error', onError),
        subscribe('task-progress-refresh-error', onRefreshError),
      ])
      if (disposed) return
      await ready()
      if (disposed) return
      await fetch()
      if (disposed) return
      // Optional legacy consumer hook only. TaskManager deliberately supplies no
      // process poll; task/progress subscriptions and refresh are independent.
      if (typeof poll === 'function') {
        poll()
        interval = setIntervalFn(poll, 600000)
      }
    } catch (error) {
      stop()
      throw error
    }
  }
  return { start, stop }
}

const taskId = (task) => typeof task._id === 'object' ? task._id?.$oid : task._id
const sameTask = (task, pending) => task && taskId(task) === pending.id &&
  (task.isRemote === true) === pending.isRemote

export function isTaskComplete(task, progress) {
  const kind = task?.taskType ?? task?.task_type
  if (kind === 'priority') {
    let config = task?.mission_config ?? task?.startConfig ?? task?.config
    try { if (typeof config === 'string') config = JSON.parse(config) } catch { return false }
    // A full intermediate round is not a completed research campaign. Its
    // bounded producer owns completion; generic five-minute auto-pause must
    // never interrupt feedback/planning or a later round.
    if (config?.rounds?.enabled === true || progress?.priority_campaign) return false
  }
  return task?.status === 'running' && progress?.stale === false &&
    progress.is_remote === (task.isRemote === true) &&
    progress.total > 0 && progress.success >= progress.total
}

// One timer per task/source, cancelled as soon as completion is no longer valid.
export function createAutoPauseScheduler({ getTasks, getProgress, pause, onError,
  delay = 5 * 60 * 1000, setTimeoutFn = setTimeout, clearTimeoutFn = clearTimeout }) {
  const pending = new Map()
  const inFlight = new Set()
  let disposed = false
  const cancel = (name) => {
    const entry = pending.get(name)
    if (entry) clearTimeoutFn(entry.timer)
    pending.delete(name)
  }
  const reconcile = () => {
    if (disposed) return
    const tasks = getTasks()
    const progress = getProgress()
    for (const [name, entry] of pending) {
      const task = tasks.find((task) => task.name === name)
      if (!sameTask(task, entry) || !isTaskComplete(task, progress[name])) cancel(name)
    }
    for (const task of tasks) {
      if (pending.has(task.name) || inFlight.has(task.name) || !isTaskComplete(task, progress[task.name])) continue
      const entry = { id: taskId(task), isRemote: task.isRemote === true }
      entry.timer = setTimeoutFn(async () => {
        // A cleared callback may already be queued. Recheck its generation too.
        if (disposed || pending.get(task.name) !== entry) return
        const current = getTasks().find((item) => item.name === task.name)
        if (!sameTask(current, entry) || !isTaskComplete(current, getProgress()[task.name])) {
          cancel(task.name)
          return
        }
        // Separate in-flight guard survives cancellation by a stale/error event.
        inFlight.add(task.name)
        try { await pause(current) } catch (error) { if (!disposed) onError(error) }
        finally {
          inFlight.delete(task.name)
          if (pending.get(task.name) === entry) pending.delete(task.name)
        }
      }, delay)
      pending.set(task.name, entry)
    }
  }
  return {
    reconcile,
    stop: () => { disposed = true; for (const name of pending.keys()) cancel(name) },
  }
}

// Propagate either command failure; callers own feedback and the single fetch.
export async function syncTaskToRemote({ sync, updateSource, refresh }) {
  await sync()
  await updateSource()
  await refresh()
}
