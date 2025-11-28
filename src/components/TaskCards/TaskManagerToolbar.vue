<template>
  <n-page-header title="回测任务管理" style="margin-bottom: 20px">
    <template #extra>
      <div style="display: flex; gap: 12px">
        <n-button
          type="primary"
          size="large"
          @click="$emit('addPriorityTask')"
          style="
            border-radius: 20px;
            padding: 10px 10px;
            font-size: 14px;
            background-color: #ff9800;
          "
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
          "
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
          "
        >
          ➕ 添加Super任务
        </n-button>
      </div>
    </template>
  </n-page-header>
</template>

<script setup>
defineEmits(['addTask', 'addSuperTask', 'addPriorityTask']);
</script>
