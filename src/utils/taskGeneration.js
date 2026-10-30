// Ordinary task generation state shared by cards and click handlers.
export function isGenerating(task, localPending = false) {
  return localPending || task?.status === 'generating';
}
export function canGenerate(task, localPending = false) {
  return !localPending && ['pending', 'waiting', 'ready', 'generation_failed'].includes(task?.status);
}
export function generationBusyOrUnknown(task, localPending = false) {
  return isGenerating(task, localPending) || task?.status === 'generation_unknown';
}

// Presentation only: the original owner persists failure separately from the
// retryable control status. lastError alone may belong to an older operation.
export function generationFailure(task, localPending = false) {
  if (generationBusyOrUnknown(task, localPending)) return null;
  const phase = task?.generation?.phase;
  const failed = phase ? phase === 'generation_failed' : task?.status === 'generation_failed';
  if (!failed) return null;
  return { status: 'generation_failed', error: typeof task?.lastError === 'string' ? task.lastError : '' };
}
