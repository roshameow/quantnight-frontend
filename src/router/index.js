import { createRouter, createWebHistory } from 'vue-router';
import TaskManager from '../views/TaskManager.vue';
import DataPage from '../views/DataPage.vue';
import Testmanager from '../views/TestManager.vue';

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
    path: '/test',
    name: 'TestManager',
    component: Testmanager
  },
];

const router = createRouter({
  history: createWebHistory(),
  routes
});

export default router;
