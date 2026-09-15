import { createApp } from 'vue';
import naive from 'naive-ui';
import Demo from './Demo.vue';

// Separate entry point: the demo does not import Tauri, stores or the desktop router.
createApp(Demo).use(naive).mount('#app');
