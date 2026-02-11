<template>
  <div class="submission-stats-container">
    <div class="header">
      <n-space align="center">
        <n-select
          v-model:value="selectedCollection"
          :options="configStore.dataFilterOptions"
          placeholder="选择数据集"
          style="width: 200px"
          @update:value="fetchStats"
        />
        <n-button type="primary" @click="fetchStats" :loading="loading">
          刷新统计
        </n-button>
      </n-space>
    </div>

    <n-data-table
      :columns="columns"
      :data="statsData"
      :loading="loading"
      :bordered="false"
      size="small"
      class="stats-table"
    />

    <n-modal
      v-model:show="showAlphaList"
      preset="card"
      :title="`Alpha 列表 - ${selectedMonth} - ${selectedRegion}`"
      style="width: 90%; max-width: 1200px"
    >
      <div style="height: 600px; overflow: auto">
        <n-data-table
          :columns="alphaColumns"
          :data="alphaListData"
          :loading="alphaLoading"
          :bordered="false"
          size="small"
        />
      </div>
    </n-modal>
  </div>
</template>

<script setup>
import { ref, onMounted, h } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { NButton, NSpace, NSelect, NDataTable, NModal } from 'naive-ui';
import { useConfigStore } from '../stores/configStore';
import { useAlphaTableColumns } from '../composables/useAlphaTableColumns';

const configStore = useConfigStore();
const loading = ref(false);
const selectedCollection = ref('alpha_results');
const statsData = ref([]);

const showAlphaList = ref(false);
const selectedMonth = ref('');
const selectedRegion = ref('');
const alphaListData = ref([]);
const alphaLoading = ref(false);

const pnlDataMap = ref({});
const loadingSet = ref(new Set());
const expandedRowIds = ref(new Set());

// Reuse alpha table columns logic if possible
const { columns: alphaColumnsRaw } = useAlphaTableColumns({
  expandedRowIds,
  pnlDataMap,
  loadingSet,
  loadPNL: async (id) => {
    if (loadingSet.value.has(id) || pnlDataMap.value[id] !== undefined) return;
    loadingSet.value.add(id);
    try {
      const result = await invoke("get_pnl_by_id", {
        query: { id, collection: selectedCollection.value },
      });
      pnlDataMap.value[id] = result.pnl_series;
    } catch (e) {
      console.error("Error loading PnL:", e);
      pnlDataMap.value[id] = null;
    } finally {
      loadingSet.value.delete(id);
    }
  }
});

// Filter out some columns for the modal list if needed
const alphaColumns = alphaColumnsRaw.filter(col => col.key !== 'pnl');

const columns = [
  {
    title: '月份',
    key: 'month',
    sorter: 'default'
  },
  {
    title: '区域',
    key: 'region',
    sorter: 'default'
  },
  {
    title: '提交个数',
    key: 'count',
    sorter: (rowA, rowB) => rowA.count - rowB.count,
    render(row) {
      return h(
        NButton,
        {
          text: true,
          type: 'primary',
          onClick: () => viewAlphaList(row.month, row.region)
        },
        { default: () => row.count }
      );
    }
  },
  {
    title: '平均 Sharpe',
    key: 'avg_sharpe',
    render: (row) => row.avg_sharpe?.toFixed(3) ?? '--',
    sorter: (rowA, rowB) => (rowA.avg_sharpe || 0) - (rowB.avg_sharpe || 0)
  },
  {
    title: '平均 Fitness',
    key: 'avg_fitness',
    render: (row) => row.avg_fitness?.toFixed(3) ?? '--',
    sorter: (rowA, rowB) => (rowA.avg_fitness || 0) - (rowB.avg_fitness || 0)
  },
  {
    title: '平均 Returns',
    key: 'avg_returns',
    render: (row) => row.avg_returns != null ? (row.avg_returns * 100).toFixed(2) + '%' : '--',
    sorter: (rowA, rowB) => (rowA.avg_returns || 0) - (rowB.avg_returns || 0)
  },
  {
    title: '平均 Turnover',
    key: 'avg_turnover',
    render: (row) => row.avg_turnover != null ? (row.avg_turnover * 100).toFixed(2) + '%' : '--',
    sorter: (rowA, rowB) => (rowA.avg_turnover || 0) - (rowB.avg_turnover || 0)
  },
  {
    title: '平均 Margin',
    key: 'avg_margin',
    render: (row) => row.avg_margin != null ? (row.avg_margin * 10000).toFixed(2) + '‱' : '--',
    sorter: (rowA, rowB) => (rowA.avg_margin || 0) - (rowB.avg_margin || 0)
  }
];

async function fetchStats() {
  loading.value = true;
  try {
    const result = await invoke('get_submission_stats', {
      collection: selectedCollection.value
    });
    statsData.value = result.map(doc => ({
      month: doc._id.month,
      region: doc._id.region,
      count: doc.count,
      avg_sharpe: doc.avg_sharpe,
      avg_fitness: doc.avg_fitness,
      avg_turnover: doc.avg_turnover,
      avg_returns: doc.avg_returns,
      avg_margin: doc.avg_margin
    }));
  } catch (e) {
    console.error('Failed to fetch stats:', e);
  } finally {
    loading.value = false;
  }
}

async function viewAlphaList(month, region) {
  selectedMonth.value = month;
  selectedRegion.value = region;
  showAlphaList.value = true;
  alphaLoading.value = true;
  alphaListData.value = [];

  try {
    // Build query to match month and region
    // Month is prefix of dateSubmitted or dateCreated
    const query = {
      $and: [
        { "settings.region": region },
        {
          $or: [
            { "dateSubmitted": { $regex: `^${month}` } },
            { $and: [{ "dateSubmitted": { $exists: false } }, { "dateCreated": { $regex: `^${month}` } }] }
          ]
        }
      ]
    };

    const params = {
      collection: selectedCollection.value,
      query: JSON.stringify(query),
      page: 1,
      page_size: 1000 // Get many for this view
    };

    const result = await invoke('get_alpha_results', { params });
    alphaListData.value = result.data;
  } catch (e) {
    console.error('Failed to fetch alpha list:', e);
  } finally {
    alphaLoading.value = false;
  }
}

onMounted(() => {
  fetchStats();
});
</script>

<style scoped>
.submission-stats-container {
  padding: 8px 0;
}
.header {
  margin-bottom: 16px;
}
.stats-table {
  background-color: #fff;
}
</style>
