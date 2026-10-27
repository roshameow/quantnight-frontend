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
  nativeStages.includes(stage) || (operation === 'runtime' && stage === 'dispatch_read') || (operation === 'stop' && ['control_write', 'inventory', 'inventory_recheck', 'stop'].includes(stage)))
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
// UI-only identity/gating. Native absence remains the final start authority.
export function taskRuntimeTarget(task) {
  return { taskId: typeof task?._id === 'object' ? task._id?.$oid : task?._id,
    name: task?.name, isRemote: task?.isRemote === true,
    authProfile: task?.auth_profile ?? 'user1', launchAttemptId: task?.launchAttemptId ?? null }
}
export function sameRuntimeIdentity(task, target) {
  const current = taskRuntimeTarget(task)
  return !!task && task.status !== 'deactive' && typeof current.taskId === 'string' && !!current.taskId &&
    ['taskId', 'name', 'isRemote', 'authProfile', 'launchAttemptId'].every(key =>
      current[key] === (key === 'launchAttemptId' ? target?.[key] ?? null : target?.[key]))
}
// Assigned by the original native list branch. Missing/unknown is NOT denial
// for legacy clients; native exact-local-ID checks remain the authority.
export const taskControlSource = task => ['local', 'remote_only'].includes(task?.controlSource)
  ? task.controlSource : 'unknown'
export const taskMutationIssue = task => taskControlSource(task) === 'remote_only'
  ? '仅远端登记：当前本地控端无此任务控制权限，请由原 owner 管理；仍可查看任务详情' : ''
export const taskControlLabel = task => taskControlSource(task) === 'local'
  ? '本控端登记（执行端可为本地或远程）' : taskMutationIssue(task) || '登记来源未提供；操作仍由原 native 精确身份核验'

export const isCompletedTask = task => ['completed', 'completed_with_errors'].includes(task?.status)
// Only view-owned admission and known control registration belong here.
// Runtime/PID snapshots and historical proof/status are never start authority.
export function taskStartBlockReason(task, { busy = false, stopping = false } = {}) {
  if (!task || !taskRuntimeTarget(task).taskId || task.status === 'deactive') return '任务身份缺失或已归档，请刷新列表'
  if (taskMutationIssue(task)) return taskMutationIssue(task)
  if (busy || stopping) return '本视图的原任务操作正在进行，请等待本次回执'
  return ''
}

export function runtimeDiagnostic(task, observation, now = Date.now()) {
  if (!observation) return { status: '尚未执行运行检查', detail: '等待首次消费者检查。' }
  const id = typeof task?._id === 'object' ? task._id?.$oid : task?._id
  if (typeof id !== 'string' || !id || observation.taskId !== id || observation.name !== task.name ||
      observation.isRemote !== (task.isRemote === true) ||
      (observation.authProfile !== undefined && observation.authProfile !== (task.auth_profile ?? 'user1')) ||
      (observation.launchAttemptId ?? null) !== (task.launchAttemptId ?? null)) {
    return { status: '运行检查身份校验失败', detail: `身份校验失败：检查结果的任务 ID、名称、执行主机、账号或启动批次与当前任务存在差异；结果已拒绝。${stageDetails(observation)}` }
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
    detail: `${context} · 进程检查仅识别到 shell/sleep 或保留的已退出 pane。` }
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
  if (isCompletedTask(task) && (!stopStatus || stopStatus === 'paused') &&
      runtimeDiagnostic(task, observation, now).status === '消费者存活') return '控制记录已完成，但原消费者仍存活'
  if (stopStatus === 'paused' && task.stopConfirmationBasis === 'session_absent') return '已暂停（tmux会话缺失）'
  if (stopStatus) return stopStatus === 'stopping' ? '停止执行中' : stopStatus
  if (isCompletedTask(task) && observation) {
    const diagnostic = runtimeDiagnostic(task, observation, now)
    return diagnostic.status === '消费者存活' ? '控制记录已完成，但原消费者仍存活' : `${task.status} · ${diagnostic.status}`
  }
  if (task.status !== 'running') {
    const historical = { unknown: '控制记录缺少执行阶段', generation_unknown: '生成结果核验缺少最终回执', launch_unknown: '启动结果核验缺少最终回执' }
    return historical[String(task.status).toLowerCase()] ?? task.status
  }
  if (!observation) return task.status // business control state; not a liveness claim
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
  if (stopped) return [stopped.detail, businessDiagnostic(task, observation, now)?.detail].filter(Boolean).join(' · ')
  if (!observation) return ''
  const diagnostic = runtimeDiagnostic(task, observation, now)
  const dispatch = regularDispatchState(task, observation, now)
  const dispatchReason = diagnostic.status === '消费者存活' && task.taskType !== 'super' && !dispatch
    ? (observation?.dispatch && now - observation.dispatch.heartbeatAt >= DISPATCH_MAX_AGE
      ? '派发快照已过期（120 秒）。'
      : '缺少身份、主机、账号和时效均有效的派发快照。') : ''
  return [isCompletedTask(task) && diagnostic.status === '消费者存活' ? '控制记录与执行状态矛盾；继续查看原任务，不重复启动。' : '', diagnostic.detail, dispatchReason, businessDiagnostic(task, observation, now)?.detail,
    !observation?.businessDiagnostic && ['not_running', 'session_absent'].includes(observation?.state)
      ? '缺少可信的历史业务错误；检查完成仅表示进程检查结束，业务完成及平台终态仍需原回执核验。' : ''].filter(Boolean).join(' · ')
}
export function nextRuntimeExpiry(task, observation, now) {
  if (!Number.isSafeInteger(observation?.observedAt)) return null
  const times = [observation.observedAt + RUNTIME_MAX_AGE]
  const dispatch = regularDispatchState(task, observation, now)
  if (dispatch) times.push(dispatch.heartbeatAt + DISPATCH_MAX_AGE)
  if (observation?.businessDiagnostic) times.push(observation.businessDiagnostic.heartbeatAt + DISPATCH_MAX_AGE)
  return Math.min(...times.filter(time => time > now))
}


export const BUSINESS_DIAGNOSTIC_MARKER = 'QUANTNIGHT_BUSINESS_DIAGNOSTIC '
export function safeDiagnosticText(value) {
  return String(value ?? '').replace(/[a-z][a-z0-9+.-]*:\/\/[^\s<>"']+/gi, '[redacted-uri]')
    .replace(/\b(?:authorization|cookie|password|passwd|token|secret|api[_-]?key)\s*[:=]\s*[^,;]+|\bBearer\s+\S+/gi, '[redacted-credential]')
    .replace(/[\x00-\x1f\x7f]/g, ' ').slice(0, 1024)
}
export function validBusinessDiagnostic(d, target, now) {
  const instant = n => Number.isSafeInteger(n) && n > 0 && n <= now
  return !!d && d.schemaVersion === 1 && d.taskId === target.taskId && d.name === target.name &&
    d.isRemote === target.isRemote && d.authProfile === target.authProfile &&
    ['priority_capability', 'simulatorRuntime', 'tmux_log'].includes(d.source) &&
    (d.source === 'tmux_log' ? d.consumerRunId === 'unavailable' && !d.launchMatched && d.launchAttemptId === null && cleanText(d.sourceDetail, 1024) : /^[a-fA-F0-9]{32}$/.test(d.consumerRunId)) &&
    (d.launchAttemptId === null || cleanText(d.launchAttemptId, 100)) && typeof d.launchMatched === 'boolean' &&
    (!d.launchMatched || (d.launchAttemptId !== null && d.launchAttemptId === target.launchAttemptId)) &&
    (!target.launchAttemptId || !d.launchAttemptId || target.launchAttemptId === d.launchAttemptId) &&
    instant(d.occurredAt) && Number.isSafeInteger(d.heartbeatAt) && d.heartbeatAt >= d.occurredAt && d.heartbeatAt <= now + 5000 &&
    stageId(d.errorClass) && stageId(d.phase) && cleanText(d.reason, 1024) &&
    ['item', 'accepted_receipts', 'new_dispatch', 'consumer'].includes(d.scope) && cleanText(d.recoveryState, 1024) &&
    (d.timeoutSeconds === null || (Number.isFinite(d.timeoutSeconds) && d.timeoutSeconds > 0 && d.timeoutSeconds <= 86400)) &&
    (d.nextRetryAt === null || (Number.isSafeInteger(d.nextRetryAt) && d.nextRetryAt >= d.occurredAt && d.nextRetryAt <= 8640000000000000))
}
export function businessDiagnostic(task, observation, now = Date.now()) {
  const target = { taskId: typeof task?._id === 'object' ? task._id?.$oid : task?._id,
    name: task.name, isRemote: task.isRemote === true, authProfile: task.auth_profile ?? 'user1',
    launchAttemptId: task.launchAttemptId ?? null }
  // Keep operation failures separate; they cannot supply business causality.
  if (!observation || observation.error || !['running', 'not_running', 'session_absent'].includes(observation.state) || observation.taskId !== target.taskId || observation.name !== target.name ||
      observation.isRemote !== target.isRemote || !Number.isSafeInteger(observation.observedAt) ||
      observation.observedAt <= 0 || observation.observedAt > now ||
      !validBusinessDiagnostic(observation.businessDiagnostic, target, now)) return null
  const d = observation.businessDiagnostic
  const current = observation.state === 'running' && d.launchMatched && task.status === 'running' &&
    now - observation.observedAt < RUNTIME_MAX_AGE && now - d.heartbeatAt < DISPATCH_MAX_AGE
  const label = current && /^(waiting|held|blocked|retry_scheduled|awaiting)/.test(d.recoveryState) ? '当前消费者错误 / 等待原恢复' : current ? '当前启动最近业务错误（恢复状态见详情）' :
    ['not_running', 'session_absent'].includes(observation.state) ? '最后历史业务错误（未证明退出因果）' :
      '历史业务错误（当前启动或时效未核验）'
  return { summary: `${label}：${d.errorClass} · ${safeDiagnosticText(d.reason)}`,
    detail: [`${label}：${d.errorClass}`, `具体原因：${safeDiagnosticText(d.reason)}`,
      `发生时间（UTC）：${new Date(d.occurredAt).toISOString()}`, `业务阶段：${d.phase}`,
      d.timeoutSeconds ? `阶段 timeout 预算：${d.timeoutSeconds} 秒` : '阶段 timeout 预算：原证据未提供',
      `影响 scope：${d.scope}`, `原恢复状态：${safeDiagnosticText(d.recoveryState)}`,
      d.nextRetryAt ? `${current && d.nextRetryAt > now ? '原有下次合法重试时间' : '历史重试计划（不是当前重试授权）'}（UTC）：${new Date(d.nextRetryAt).toISOString()}`
        : '下次合法重试：原证据未提供；诊断不安排重试',
      `来源：${d.source}${d.sourceDetail ? ` (${safeDiagnosticText(d.sourceDetail)})` : ''} · consumer=${d.consumerRunId} · launch=${d.launchAttemptId ?? '缺失'}`,
      '进程检查完成不代表业务完成；历史错误不自动证明当前退出原因。'].join(' · ') }
}
export function taskDiagnosticSummary(task, observation, now = Date.now()) {
  const business = businessDiagnostic(task, observation, now)
  if (business) return business.summary
  const runtime = runtimeDiagnostic(task, observation, now)
  if (observation?.error) return `${runtime.status}：${safeDiagnosticText(observation.error)}`
  const dispatch = regularDispatchState(task, observation, now)
  if (runtime.status === '消费者存活' && dispatch?.submissionReason) return `派发限制：${safeDiagnosticText(dispatch.submissionReason)}`
  if (['not_running', 'session_absent'].includes(observation?.state)) return '缺少可归因的历史业务错误证据（不根据末条日志推断退出原因）'
  return ''
}


// Original TaskOutcome decoding only. Never extract IDs/launch from free text.
// Null/undefined acknowledge an old void adapter but supply NO join/launch proof.
export function taskStartOutcome(value, target) {
  if (value == null) return null
  if (!value || typeof value !== 'object' || Array.isArray(value) || value.schemaVersion !== 1 ||
      value.command !== 'start' || value.task !== target.name || typeof value.ok !== 'boolean') {
    throw new Error('启动回执格式/任务名称无效；未认定成功')
  }
  const matchedHost = value.remote === target.remote || (value.ok === false &&
    value.requestSatisfied === false && value.remote === (target.actualRemote ?? target.remote))
  const identityMatches = value.taskId === target.taskId && matchedHost
  if ((value.taskId !== undefined && value.taskId !== target.taskId) ||
      (value.remote !== undefined && !matchedHost) ||
      ((value.ok || value.consumerRunning === true) && !identityMatches)) {
    throw new Error('启动回执身份/执行端不符；未认定成功或消费者归属')
  }
  // Refusal before configuration comparison may omit that comparison. It is
  // still a failure, never a positive consumer/launch attribution.
  if (value.ok === false && value.outcome === 'refused' && value.consumerRunning !== true &&
      value.startedNew === false && value.configApplied === false && value.requestSatisfied === false) return value
  if (value.outcome !== undefined) {
    if (!['started', 'already_running', 'refused'].includes(value.outcome) ||
        typeof value.startedNew !== 'boolean' || typeof value.configApplied !== 'boolean' ||
        typeof value.requestSatisfied !== 'boolean' ||
        !['equal', 'different', 'unverified', 'not_requested', 'not_compared'].includes(value.configComparison)) {
      throw new Error('启动结构化回执字段无效；未认定成功')
    }
    if (value.ok && (!value.requestSatisfied || value.outcome === 'refused')) throw new Error('启动回执成功/意图结果矛盾')
    if (value.outcome === 'already_running' &&
        (value.startedNew || value.configApplied || value.consumerRunning !== true || !identityMatches ||
         (value.ok && !['equal', 'not_requested', 'not_compared'].includes(value.configComparison)))) {
      throw new Error('已运行回执矛盾；未认定配置应用或消费者归属')
    }
    if (value.outcome === 'started' && value.ok && !value.startedNew) throw new Error('启动回执未确认新启动')
  }
  return value
}
export function taskStartFailure(error, target) {
  let value = error
  if (typeof value === 'string') {
    try { value = JSON.parse(value) } catch { return null }
  }
  try {
    const result = taskStartOutcome(value, target)
    return result?.ok === false ? result : null
  } catch { return null }
}


// Missing/null are a real absence expectation; other types are not absence.
export function taskLaunchValue(task) {
  if (!Object.hasOwn(task ?? {}, 'launchAttemptId') || task.launchAttemptId === null) return null
  return typeof task.launchAttemptId === 'string' && task.launchAttemptId.trim() ? task.launchAttemptId : undefined
}
export function taskLaunchMatches(task, expected) {
  if (expected?.expectedLaunchAbsent === true && expected.expectedLaunchAttemptId != null) return false
  if (expected?.expectedLaunchAbsent === true) return taskLaunchValue(task) === null
  if (expected?.expectedLaunchAttemptId != null) return typeof expected.expectedLaunchAttemptId === 'string' &&
    !!expected.expectedLaunchAttemptId.trim() && taskLaunchValue(task) === expected.expectedLaunchAttemptId
  return true // old direct caller: omission is NOT must-be-absent
}
export function taskStartJoinEvidence(value, binding) {
  const outcome = taskStartOutcome(value, binding)
  if (!outcome || outcome.operationSettled !== true || !['started', 'already_running', 'refused'].includes(outcome.outcome) ||
      outcome.taskId !== binding.taskId || outcome.task !== binding.name ||
      typeof outcome.remote !== 'boolean' || outcome.taskType !== binding.taskType || outcome.authProfile !== binding.authProfile ||
      !Object.hasOwn(outcome, 'launchAttemptId') || taskLaunchValue(outcome) === undefined) {
    throw new Error('原启动缺少已收敛的精确身份/launch回执；本次暂停未发送')
  }
  return { taskId: outcome.taskId, name: outcome.task, isRemote: outcome.remote,
    taskType: outcome.taskType, authProfile: outcome.authProfile, launchAttemptId: outcome.launchAttemptId }
}
export function taskMatchesJoinEvidence(task, evidence) {
  return !!task && task.status !== 'deactive' && !taskMutationIssue(task) &&
    taskRuntimeTarget(task).taskId === evidence.taskId && task.name === evidence.name &&
    (task.taskType ?? 'regular') === evidence.taskType && (task.auth_profile ?? 'user1') === evidence.authProfile &&
    (task.isRemote === true) === evidence.isRemote && taskLaunchValue(task) === evidence.launchAttemptId
}
