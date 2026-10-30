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

// Propagate either command failure; callers own feedback and the single fetch.
export async function syncTaskToRemote({ sync, updateSource, refresh }) {
  await sync()
  await updateSource()
  await refresh()
}
