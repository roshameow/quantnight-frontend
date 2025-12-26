import { createRouter, createWebHistory } from 'vue-router';
import TaskManager from '../views/TaskManager.vue';
import DataPage from '../views/DataPage.vue';
import SettingsView from '../views/SettingsView.vue';

const routes = [
  {
    path: '/',
    redirect: '/task'
  },
  {
    path: '/task',
    name: 'TaskManager',
    component: TaskManager
  },
  {
    path: '/data',
    name: 'Data',
    component: DataPage
  },
  {
    path: '/settings',
    name: 'Settings',
    component: SettingsView
  },
];

const router = createRouter({
  history: createWebHistory(),
  routes
});

export default router;
