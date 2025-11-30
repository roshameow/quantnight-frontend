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
defineEmits(['addTask', 'addSuperTask', 'addPriorityTask']);
</script>
