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

  // 全局监听 Rust emit 的任务进度
  await listen("task-progress-update", (event) => {
    // 写入 Pinia store
    taskStore.updateProgress(event.payload);
  });

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
initGlobalTaskWatcher();
