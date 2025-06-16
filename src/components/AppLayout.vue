<template>
  <n-layout>
    <n-layout-header bordered style="padding: 16px; background: #f5f5f5;">
      <n-space justify="space-between" align="center">
        <div class="text-xl font-bold">🧠 QuantNight 回测平台</div>
        <n-button tertiary @click="handleSettings">设置</n-button>
      </n-space>
    </n-layout-header>

    <n-layout has-sider style="height: calc(100vh - 64px);">
      <n-layout-sider bordered width="200">
        <n-menu
          :options="menuOptions"
          :default-value="'/task'"
          @update:value="handleMenuClick"
        />
      </n-layout-sider>

      <n-layout-content style="padding: 16px;">
        <slot />
      </n-layout-content>
    </n-layout>
  </n-layout>
</template>

<script setup>
import { ref } from 'vue'
import { useRouter } from 'vue-router'

const router = useRouter()

const menuOptions = [
  {
    label: '回测任务管理',
    key: '/task',
  },
  {
    label: '策略构建',
    key: '/strategy',
  },
  {
    label: '数据面板',
    key: '/data',
  },
  {
    label: '测试巡游',
    key: '/test',
  },
]

function handleMenuClick(key) {
  router.push(key)
}

function handleSettings() {
  console.log('打开设置')
}
</script>

<style scoped>
.text-xl {
  font-size: 20px;
}
</style>
