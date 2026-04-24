<template>
  <div class="data-page-container">
    <n-tabs type="line" animated>
      <n-tab-pane name="table" tab="Alpha 列表">
        <AlphaTablePanel />
      </n-tab-pane>
      <n-tab-pane name="analysis" tab="数据分析">
        <AnalysisPanel />
      </n-tab-pane>
      <n-tab-pane name="stats" tab="提交统计">
        <SubmissionStatsPanel />
      </n-tab-pane>
      <n-tab-pane name="datasets" tab="数据集">
        <DatasetPanel />
      </n-tab-pane>
    </n-tabs>
  </div>
</template>

<script setup>
import { NTabs, NTabPane } from "naive-ui";
import AlphaTablePanel from "./AlphaTablePanel.vue";
import AnalysisPanel from "./AnalysisPanel.vue";
import SubmissionStatsPanel from "./SubmissionStatsPanel.vue";
import DatasetPanel from "./DatasetPanel.vue";
</script>

<style scoped>
.data-page-container {
  padding: 16px;
  background-color: #fff;
  min-height: 100vh;
}
</style>