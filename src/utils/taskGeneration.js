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
