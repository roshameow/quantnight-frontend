<script setup>
import { computed, ref } from 'vue';
import RegularTaskCard from '../components/TaskCards/RegularTaskCard.vue';
import PriorityTaskCard from '../components/TaskCards/PriorityTaskCard.vue';

const examples = [
  { name: 'demo_value_research', taskType: 'regular', status: 'running', isRemote: false, auth_profile: 'demo', mission_config: { max_concurrent: 3, max_multi_simulation_children: 10 }, progress: { success: 72, error: 3, total: 120 } },
  { name: 'demo_news_screen', taskType: 'regular', status: 'ready', isRemote: true, auth_profile: 'demo', progress: { success: 0, error: 0, total: 60 } },
  { name: 'demo_signal_combo', taskType: 'super', status: 'done', isRemote: true, auth_profile: 'demo', progress: { success: 48, error: 2, total: 50 } },
  { name: 'demo_priority_check', taskType: 'priority', status: 'running', isRemote: false, auth_profile: 'demo', mission_config: { priority: 1 }, progress: { success: 12, error: 0, total: 30 } },
];
const tasks = ref(structuredClone(examples));
const note = ref('所有数据均为合成示例。按钮只修改当前页面，刷新或重置即可恢复。');
const region = ref(null);
const query = ref('');
const results = [
  { id: 'DEMO-001', region: 'USA', family: 'Value example', sharpe: 1.25, turnover: '18%', status: 'review' },
  { id: 'DEMO-002', region: 'GLB', family: 'News example', sharpe: 0.82, turnover: '26%', status: 'review' },
  { id: 'DEMO-003', region: 'USA', family: 'Combo example', sharpe: 1.10, turnover: '14%', status: 'review' },
];
const filtered = computed(() => results.filter(row => (!region.value || row.region === region.value) && `${row.id} ${row.family}`.toLowerCase().includes(query.value.toLowerCase())));
const columns = [{title:'ID',key:'id'}, {title:'Region',key:'region'}, {title:'研究类别',key:'family'}, {title:'示例 Sharpe',key:'sharpe'}, {title:'示例 Turnover',key:'turnover'}, {title:'状态',key:'status'}];
function update(task, status) { task.status = status; note.value = `${task.name}: ${status}（仅演示，没有执行后端任务）`; }
function reset() { tasks.value = structuredClone(examples); note.value = '示例已重置。'; }
function remove(task) { tasks.value = tasks.value.filter(t => t !== task); note.value = '已移除当前页面的示例任务，可随时重置。'; }
function remote(task, value) { task.isRemote = value; note.value = '仅切换示例标记，没有连接远程主机。'; }
function inspect(task) { query.value = ''; note.value = `${task.name} · ${task.taskType} · ${task.status} · ${task.progress.success}/${task.progress.total}`; }
</script>

<template>
  <n-config-provider>
    <main class="demo-page">
      <header><div><p class="eyebrow">INTERACTIVE PREVIEW · SYNTHETIC DATA</p><h1>QuantNight</h1><p>量化研究任务与结果，一处查看。</p></div><n-button @click="reset">重置示例</n-button></header>
      <n-alert type="info" :show-icon="false" style="margin-bottom:28px">{{ note }}</n-alert>
      <h2>任务管理</h2>
      <p class="section-note">复用桌面应用的任务卡片。启动、暂停、本地/远程切换均为内存演示。</p>
      <div class="task-grid">
        <component v-for="task in tasks" :key="task.name" :is="task.taskType === 'priority' ? PriorityTaskCard : RegularTaskCard" :task="task" :progress="task.progress" :task-runtime-status="task.status" @start="update(task,'running')" @pause="update(task,'ready')" @delete="remove(task)" @update:is-remote="value => remote(task,value)" @view="inspect(task)" @update="inspect(task)" @generate-list="update(task,'ready')" @sync-remote="remote(task,true)" />
      </div>
      <h2>结果浏览</h2>
      <p class="section-note">以下数字仅用于界面演示，不代表策略表现。</p>
      <n-space style="margin-bottom:16px"><n-input v-model:value="query" placeholder="搜索示例 ID 或类别" clearable /><n-select v-model:value="region" placeholder="全部 Region" clearable :options="[{label:'USA',value:'USA'},{label:'GLB',value:'GLB'}]" style="width:180px" /></n-space>
      <n-data-table :columns="columns" :data="filtered" :bordered="false" />
      <footer><a href="https://github.com/roshameow/quantnight-frontend">源码与完整桌面安装说明 ↗</a></footer>
    </main>
  </n-config-provider>
</template>

<style>
body{margin:0;background:#f6f8fa;font-family:system-ui,-apple-system,sans-serif;color:#20333b}.demo-page{max-width:1320px;margin:0 auto;padding:40px 28px}header{display:flex;justify-content:space-between;align-items:center;gap:20px}.eyebrow{letter-spacing:.13em;font-size:11px;color:#547970}h1{font-size:36px;margin:4px 0}h2{font-size:19px;margin:30px 0 8px}.section-note{color:#657983;font-size:13px;margin:0 0 18px}.task-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(270px,1fr));gap:20px}.task-grid .task-card{width:100%;max-width:none!important;box-sizing:border-box}footer{margin-top:32px;font-size:13px}footer a{color:#246858}a:focus-visible{outline:2px solid #246858;outline-offset:4px}@media(max-width:600px){.demo-page{padding:22px 16px}header{align-items:flex-start}h1{font-size:30px}}
</style>
