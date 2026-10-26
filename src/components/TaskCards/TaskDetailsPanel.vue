<template>
  <section class="task-details" aria-label="任务详情">
    <h3>任务详情</h3>
    <div>任务 ID：{{ taskId }}</div>
    <p v-if="!task">未找到当前活动记录；可能已归档、移除或尚未加载。不会按同名任务认领替代记录。</p>
    <template v-else>
      <div>任务名：{{ task.name }}</div>
      <div>类型：{{ task.taskType || 'regular' }} · 实际执行端：{{ task.isRemote === true ? '远程' : '本地' }}</div>
      <div>控制登记：{{ taskControlLabel(task) }}</div>
      <div v-if="task.status === 'deactive'">该记录已归档，不代表平台请求已取消。</div>
      <div>控制状态：{{ task.status }}</div>
      <div>运行 / 停止观察：{{ regularDisplayStatus(task, runtime, taskStopDisplayStatus(task, stopOperation)) }}</div>
      <div>最后观察：{{ observationTime }}</div>
      <p v-if="diagnostic">{{ diagnostic }}</p>
      <div>已知本地计数：成功 {{ count('success') }} / 错误 {{ count('error') }} / 总计 {{ count('total') }}</div>
      <div v-if="progress?.stale">进度尚未核验或为缓存：{{ progress.lastError || '等待原进度更新' }}</div>
      <p>未决 / 已接受回执明细：当前摘要未提供。计数、暂停或进程退出不等于平台终态。</p>
      <p>本页是任务详情，不是任务结果分页。任务分页结果使用原 taskctl results --name {{ task.name }} --limit 20。</p>
      <a href="/data" @click.prevent="$emit('browseAlphaLibrary')">浏览正式 Alpha 全库（不是本任务完整结果）</a>
    </template>
  </section>
</template>

<script setup>
import { computed } from 'vue';
import { taskControlLabel, regularDisplayStatus, taskDiagnosticDetails } from '../../utils/taskDiagnostics.js';
import { taskStopDisplayStatus, taskStopProofIssue } from '../../stores/taskStore.js';
const props = defineProps({ taskId: String, task: Object, progress: Object, runtime: Object, stopOperation: Object });
defineEmits(['browseAlphaLibrary']);
const diagnostic = computed(() => props.task ? taskDiagnosticDetails(props.task, props.runtime, props.stopOperation,
  taskStopDisplayStatus(props.task, props.stopOperation), Date.now(), taskStopProofIssue(props.task)) : '');
const observationTime = computed(() => Number.isSafeInteger(props.runtime?.observedAt) && props.runtime.observedAt > 0 && props.runtime.observedAt <= 8640000000000000
  ? new Date(props.runtime.observedAt).toISOString() : '未提供有效观察时间');
const count = (key) => {
  if (!props.progress || (props.progress.stale && props.progress.updated_at == null)) return '未提供';
  return Number.isSafeInteger(props.progress[key]) && props.progress[key] >= 0 ? props.progress[key] : '未提供';
};
</script>
