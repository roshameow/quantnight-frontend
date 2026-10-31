<template>
  <n-page-header style="margin-bottom: 20px">
    <template #title>
      <span style="display: inline-flex; align-items: center; gap: 6px">
        <span>回测任务管理</span>
        <n-button quaternary circle size="small" :loading="refreshing" :disabled="refreshing"
          :title="refreshing ? '刷新中…' : '刷新任务'" aria-label="刷新任务" :aria-busy="refreshing"
          @click="$emit('refresh')">
          <template #icon>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
              stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M3 12a9 9 0 0 1 15.36-6.36L21 8M21 3v5h-5" />
              <path d="M21 12a9 9 0 0 1-15.36 6.36L3 16M8 16H3v5" />
            </svg>
          </template>
        </n-button>
      </span>
    </template>
    <template #extra>
      <div style="display: flex; flex-wrap: wrap; gap: 12px">
        <n-button
          type="primary"
          size="large"
          @click="$emit('addPriorityTask')"
          style="
            border-radius: 20px;
            padding: 10px 10px;
            font-size: 14px;
            background-color: #ff9800;
            box-shadow: 0 2px 4px rgba(255, 152, 0, 0.2);
            transition: all 0.2s ease;
          "
          @mouseenter="$event.target.style.transform = 'translateY(-1px)'"
          @mouseleave="$event.target.style.transform = 'translateY(0)'"
        >
          ➕ 添加Priority任务
        </n-button>
        <n-button
          type="primary"
          size="large"
          @click="$emit('addTask')"
          style="
            border-radius: 20px;
            padding: 10px 10px;
            font-size: 14px;
            background-color: #4caf50;
            box-shadow: 0 2px 4px rgba(76, 175, 80, 0.2);
            transition: all 0.2s ease;
          "
          @mouseenter="$event.target.style.transform = 'translateY(-1px)'"
          @mouseleave="$event.target.style.transform = 'translateY(0)'"
        >
          ➕ 添加任务
        </n-button>
        <n-button
          type="primary"
          size="large"
          @click="$emit('addSuperTask')"
          style="
            border-radius: 20px;
            padding: 10px 10px;
            font-size: 14px;
            background-color: #2196f3;
            box-shadow: 0 2px 4px rgba(33, 150, 243, 0.2);
            transition: all 0.2s ease;
          "
          @mouseenter="$event.target.style.transform = 'translateY(-1px)'"
          @mouseleave="$event.target.style.transform = 'translateY(0)'"
        >
          ➕ 添加Super任务
        </n-button>
      </div>
    </template>
  </n-page-header>
</template>

<script setup>
defineProps({ refreshing: { type: Boolean, default: false } });
defineEmits(['addTask', 'addSuperTask', 'addPriorityTask', 'refresh']);
</script>
