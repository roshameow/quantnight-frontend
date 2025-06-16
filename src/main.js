import { createApp } from 'vue';
import App from './App.vue';
import router from './router';
import naive from 'naive-ui';
import { createPinia } from 'pinia'; // 持久化任务


const app = createApp(App);
app.use(router);
app.use(naive);
app.use(createPinia()) // 注册 Pinia
app.mount('#app');

