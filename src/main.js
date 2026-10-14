// src/main.js
import { createApp } from 'vue';
import App from './App.vue';
import router from './router';
import naive from 'naive-ui';
import { createPinia } from 'pinia';

// ⬇️ 新增：引入 tauri 事件 API 和 store
import { listen } from '@tauri-apps/api/event';
import { useTaskStore } from './stores/taskStore';

async function initGlobalTaskWatcher() {
  const taskStore = useTaskStore();

  // Keep both success and stale/error state current even outside the task page.
  await Promise.all([
    listen("task-progress-update", (event) => taskStore.updateProgress(event.payload)),
    listen("task-progress-error", (event) => taskStore.updateProgressError(event.payload)),
    listen("task-progress-refresh-error", (event) => taskStore.markProgressStale(event.payload)),
  ]);

  console.log("✅ 全局任务进度监听器已启动");
}

// ---------------------

const app = createApp(App);

// 注册插件
app.use(router);
app.use(naive);
app.use(createPinia());

app.mount('#app');

// ⬇️ 在 app 挂载之后启动全局 watcher
initGlobalTaskWatcher().catch((error) => console.error("启动全局任务进度监听失败", error));
