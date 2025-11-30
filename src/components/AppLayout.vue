<template>
  <n-layout>
    <n-layout-header bordered class="app-header">
      <n-space justify="space-between" align="center">
        <div class="app-title">
          <div class="title-text">
            <span class="brand">QuantNight</span>
            <span class="subtitle">量化回测平台</span>
          </div>
        </div>
        <n-button tertiary @click="showSettingsModal = true">设置</n-button>
      </n-space>
    </n-layout-header>

    <n-layout has-sider style="height: calc(100vh - 49px);">
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

    <SettingsModal v-model:show="showSettingsModal" />
  </n-layout>
</template>

<script setup>
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { NLayout, NLayoutHeader, NLayoutSider, NLayoutContent, NSpace, NButton, NMenu } from 'naive-ui'
import SettingsModal from './modals/SettingsModal.vue'

const router = useRouter()
const showSettingsModal = ref(false)

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
</script>

<style scoped>
.app-header {
  padding: 12px 24px;
  background: #ffffff;
  border-bottom: 1px solid #e8e8e8;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.05);
}

.app-title {
  display: flex;
  align-items: center;
}

.title-text {
  display: flex;
  flex-direction: column;
}

.brand {
  font-size: 20px;
  font-weight: 600;
  color: #1a1a1a;
  letter-spacing: -0.5px;
}

.subtitle {
  font-size: 12px;
  color: #666666;
  font-weight: 400;
  margin-top: 1px;
}
</style>
