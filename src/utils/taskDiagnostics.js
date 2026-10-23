// Presentation/diagnostic metadata only. None of these values is execution,
// dispatch, platform-terminal or stop proof; the original receipt gates own that.
export const RUNTIME_MAX_AGE = 600000
export const DISPATCH_MAX_AGE = 120000
export const TASK_STAGE_MARKER = 'QUANTNIGHT_TASK_STAGE '
const cleanText = (value, max) => typeof value === 'string' && value.trim().length > 0 &&
  value.length <= max && !/[\x00-\x1f\x7f]/.test(value)
const stageId = value => typeof value === 'string' && /^[a-z][a-z0-9_]{0,63}$/.test(value)
// Fixed IDs shared with the existing native bridge/backend diagnostic protocol.
export const TASK_SCRIPT_STAGES = Object.freeze({
  runtime: Object.freeze(['config', 'ssh', 'remote_environment', 'tmux', 'process', 'receipt', 'session_absent', 'complete']),
  stop: Object.freeze(['config', 'ssh', 'remote_environment', 'tmux', 'process', 'interrupt', 'session_exit', 'process_exit', 'receipt', 'session_absent', 'complete']),
})
const scriptStages = TASK_SCRIPT_STAGES
const nativeStages = ['initial', 'identity', 'config', 'launch', 'wait', 'receipt_validate', 'completed']
const knownStage = (operation, stage) => stageId(stage) && (scriptStages[operation]?.includes(stage) ||
  nativeStages.includes(stage) || (operation === 'runtime' && stage === 'dispatch_read') || (operation === 'stop' && stage === 'control_write'))
let sequence = 0
export function newTaskRequestId(operation) {
  return `${operation}:${globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`}:${++sequence}`
}

export function validOperationStage(event) {
  return !!event && ['runtime', 'stop'].includes(event.operation) &&
    cleanText(event.taskId, 200) && cleanText(event.name, 512) && typeof event.isRemote === 'boolean' &&
    cleanText(event.requestId, 200) && knownStage(event.operation, event.stage) && cleanText(event.message, 4096)
}
export function validRuntimeStage(value, target) {
  return !!value && Object.keys(value).length === 5 && value.operation === 'runtime' && value.task_name === target.name &&
    typeof value.target_remote === 'boolean' && value.target_remote === target.isRemote &&
    scriptStages.runtime.includes(value.stage) && cleanText(value.message, 1024)
}
export function stageDetails(value) {
  return value?.stage ? `最后到达阶段 [${value.stage}]：${value.detail || '该阶段说明缺失'}`
    : '最后到达阶段：等待原生阶段回报'
}
export function failureKind(error) {
  return /timeout|timed out|超时/i.test(String(error)) ? 'timeout' : 'error'
}
export function errorStage(error) {
  const match = String(error).match(/(?:^\s*(?:Error:\s*)?\[|\b(?:runtime|stop) stage=)([a-z][a-z0-9_]{0,63})(?:\]|（)/)
  return match?.[1] ?? null
}
export function stopDiagnostic(task, operation, stopStatus, proofIssue = '') {
  if (!stopStatus) return null
  const id = typeof task?._id === 'object' ? task._id?.$oid : task?._id
  if (!operation || operation.name !== task.name || operation.isRemote !== (task.isRemote === true) ||
      (typeof operation._id === 'object' ? operation._id?.$oid : operation._id) !== id) operation = null
  if (stopStatus === 'paused') {
    const basis = operation?.receipt?.confirmationBasis ?? task.stopConfirmationBasis ?? 'runners_exited'
    const evidence = basis === 'session_absent'
      ? '已暂停（tmux会话缺失）：已核验目标主机的精确会话缺失、任务身份、执行目标及暂停记录回写。'
      : '停止凭据已核验：任务身份、执行目标、原记录执行进程退出及控制记录写入均匹配。'
    return { status: 'paused', detail: `${operation ? stageDetails(operation) : '凭据来源：持久化暂停核验记录'} · ${evidence}` }
  }
  if (stopStatus === '暂停标记无有效停止凭据') return { status: stopStatus,
    detail: `控制记录为 paused；${proofIssue || '缺少绑定当前任务身份/执行主机的有效停止凭据'}。请通过原停止入口核验。` }
  // The caller's stopStatus has already applied the store's identity/host gates.
  const status = stopStatus === 'stopping' ? '停止执行中' : stopStatus
  const evidence = /执行退出已核验/.test(`${operation?.detail || ''} ${operation?.error || ''}`)
    ? '执行退出已核验' : '暂停依据已核验'
  return { status, detail: [stageDetails(operation), operation?.stage === 'control_write' && operation?.status === 'stop_unconfirmed'
    ? (operation.failureSource === 'native' && operation.failureKind !== 'timeout'
      ? `${evidence}；control_write 控制记录写入失败（原生调用返回错误）。`
      : `${evidence}，已进入 control_write；control_write 最终写入结果待核对。`) : '', operation?.error ? `原因：${operation.error}`
    : operation?.status === 'stopping' ? '正在等待原生停止核验及本次停止凭据。'
      : '本次停止操作缺少有效最终回执。'].filter(Boolean).join(' · ') }
}
export function runtimeDiagnostic(task, observation, now = Date.now()) {
  if (!observation) return { status: '尚未执行运行检查', detail: '等待首次消费者检查。' }
  const id = typeof task?._id === 'object' ? task._id?.$oid : task?._id
  if (typeof id !== 'string' || !id || observation.taskId !== id || observation.name !== task.name ||
      observation.isRemote !== (task.isRemote === true) ||
      (observation.authProfile !== undefined && observation.authProfile !== (task.auth_profile ?? 'user1'))) {
    return { status: '运行检查身份校验失败', detail: `身份校验失败：检查结果的任务 ID、名称、执行主机或账号与当前任务存在差异；结果已拒绝。${stageDetails(observation)}` }
  }
  if (!Number.isSafeInteger(observation.observedAt) || observation.observedAt <= 0 || observation.observedAt > now) {
    return { status: '运行检查时间无效', detail: `结果时间戳缺失、无效或位于未来。${stageDetails(observation)}` }
  }
  if (now - observation.observedAt >= RUNTIME_MAX_AGE) return { status: '运行检查结果已过期',
    detail: `运行检查快照已达到 ${RUNTIME_MAX_AGE / 1000} 秒有效期。${stageDetails(observation)}${observation.error ? ` · 原因：${observation.error}` : observation.state === 'unknown'
      ? ` · 原检查诊断：${observation.diagnostic || ''}${observation.raw ? ` · 原始输出：${observation.raw}` : ''}` : ''}` }
  const context = stageDetails(observation)
  if (observation.state === 'checking') return { status: '运行检查进行中', detail: `${context} · 正等待本次检查结果。` }
  if (observation.error) return { status: failureKind(observation.error) === 'timeout' ? '运行检查超时' : '运行检查执行失败',
    detail: `${context} · 原因：${observation.error}` }
  if (observation.state === 'session_absent') return { status: '已暂停（tmux会话缺失）',
    detail: `${context} · 已核验目标主机的精确 tmux 会话缺失。` }
  if (observation.state === 'not_running') return { status: '未检测到运行消费者',
    detail: `${context} · 进程检查仅识别到 shell/sleep。` }
  if (observation.state === 'running') return { status: '消费者存活', detail: `${context} · 已识别 Python/uv 消费者进程。` }
  return { status: '运行检查输出校验失败', detail: `${context} · ${observation.diagnostic || '检查输出缺少可校验的 Python/uv 或 shell/sleep 进程行。'}${observation.raw ? ` · 原始诊断：${observation.raw}` : ''}` }
}
export function regularDispatchState(task, observation, now = Date.now()) {
  const d = observation?.dispatch
  const id = typeof task._id === 'object' ? task._id?.$oid : task._id
  if (task.taskType === 'super' || !d || d.schemaVersion !== 1 || d.taskId !== id || d.name !== task.name ||
      d.isRemote !== (task.isRemote === true) || d.authProfile !== (task.auth_profile ?? 'user1') ||
      !Number.isSafeInteger(d.heartbeatAt) || d.heartbeatAt > now + 5000 || now - d.heartbeatAt >= DISPATCH_MAX_AGE) return null
  return d
}
export function regularDisplayStatus(task, observation, stopStatus, now = Date.now()) {
  if (stopStatus === 'paused' && task.stopConfirmationBasis === 'session_absent') return '已暂停（tmux会话缺失）'
  if (stopStatus) return stopStatus === 'stopping' ? '停止执行中' : stopStatus
  if (task.status !== 'running') {
    const historical = { unknown: '控制记录缺少执行阶段', generation_unknown: '生成结果核验缺少最终回执', launch_unknown: '启动结果核验缺少最终回执' }
    return historical[String(task.status).toLowerCase()] ?? task.status
  }
  const diagnostic = runtimeDiagnostic(task, observation, now)
  if (diagnostic.status !== '消费者存活') return diagnostic.status
  const dispatch = regularDispatchState(task, observation, now)
  if (dispatch?.submissionState === 'waiting_reconciliation') return '请求结果待核验（派发暂停，消费者存活）'
  if (dispatch?.submissionState === 'waiting_quota_reset') return '等待配额恢复（派发暂停，消费者存活）'
  if (dispatch?.submissionState === 'ready') return '消费者存活（可派发）'
  return task.taskType === 'super' ? 'running' : '消费者存活（缺少有效派发快照）'
}
export function taskDiagnosticDetails(task, observation, operation, stopStatus, now = Date.now(), proofIssue = '') {
  const stopped = stopDiagnostic(task, operation, stopStatus, proofIssue)
  if (stopped) return stopped.detail
  if (task.status !== 'running') return ''
  const diagnostic = runtimeDiagnostic(task, observation, now)
  const dispatch = regularDispatchState(task, observation, now)
  const dispatchReason = diagnostic.status === '消费者存活' && task.taskType !== 'super' && !dispatch
    ? (observation.dispatch && now - observation.dispatch.heartbeatAt >= DISPATCH_MAX_AGE
      ? '派发快照已过期（120 秒）。'
      : '缺少身份、主机、账号和时效均有效的派发快照。') : ''
  return [diagnostic.detail, dispatchReason].filter(Boolean).join(' · ')
}
export function nextRuntimeExpiry(task, observation, now) {
  if (!Number.isSafeInteger(observation?.observedAt)) return null
  const times = [observation.observedAt + RUNTIME_MAX_AGE]
  const dispatch = regularDispatchState(task, observation, now)
  if (dispatch) times.push(dispatch.heartbeatAt + DISPATCH_MAX_AGE)
  return Math.min(...times.filter(time => time > now))
}
