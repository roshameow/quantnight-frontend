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
      :row-key="(row) => row.key"
    />

    <n-modal
      v-model:show="showAlphaList"
      preset="card"
      :title="`Alpha 列表 - ${selectedMonth} ${selectedRegion ? '- ' + selectedRegion : ''}`"
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
    title: '时间/区域',
    key: 'label',
    render(row) {
      return row.region || row.month;
    }
  },
  {
    title: '提交个数',
    key: 'count',
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
    render: (row) => row.avg_sharpe?.toFixed(3) ?? '--'
  },
  {
    title: '平均 Fitness',
    key: 'avg_fitness',
    render: (row) => row.avg_fitness?.toFixed(3) ?? '--'
  },
  {
    title: '平均 Returns',
    key: 'avg_returns',
    render: (row) => row.avg_returns != null ? (row.avg_returns * 100).toFixed(2) + '%' : '--'
  },
  {
    title: '平均 Turnover',
    key: 'avg_turnover',
    render: (row) => row.avg_turnover != null ? (row.avg_turnover * 100).toFixed(2) + '%' : '--'
  },
  {
    title: '平均 Margin',
    key: 'avg_margin',
    render: (row) => row.avg_margin != null ? (row.avg_margin * 10000).toFixed(2) + '‱' : '--'
  }
];

async function fetchStats() {
  loading.value = true;
  try {
    const result = await invoke('get_submission_stats', {
      collection: selectedCollection.value
    });
    
    // Group by month
    const monthGroups = {};
    result.forEach(doc => {
      const month = doc._id.month || 'Unknown';
      if (!monthGroups[month]) {
        monthGroups[month] = {
          month,
          count: 0,
          avg_sharpe: 0,
          avg_fitness: 0,
          avg_turnover: 0,
          avg_returns: 0,
          avg_margin: 0,
          children: []
        };
      }
      
      const regionData = {
        key: `${month}-${doc._id.region}`,
        month,
        region: doc._id.region,
        count: doc.count,
        avg_sharpe: doc.avg_sharpe,
        avg_fitness: doc.avg_fitness,
        avg_turnover: doc.avg_turnover,
        avg_returns: doc.avg_returns,
        avg_margin: doc.avg_margin
      };
      
      monthGroups[month].children.push(regionData);
      
      // Accumulate totals for the month row
      monthGroups[month].count += doc.count;
      monthGroups[month].avg_sharpe += (doc.avg_sharpe || 0) * doc.count;
      monthGroups[month].avg_fitness += (doc.avg_fitness || 0) * doc.count;
      monthGroups[month].avg_turnover += (doc.avg_turnover || 0) * doc.count;
      monthGroups[month].avg_returns += (doc.avg_returns || 0) * doc.count;
      monthGroups[month].avg_margin += (doc.avg_margin || 0) * doc.count;
    });

    // Finalize averages and convert to array
    statsData.value = Object.values(monthGroups).map(m => {
      if (m.count > 0) {
        m.avg_sharpe /= m.count;
        m.avg_fitness /= m.count;
        m.avg_turnover /= m.count;
        m.avg_returns /= m.count;
        m.avg_margin /= m.count;
      }
      return {
        ...m,
        key: m.month
      };
    }).sort((a, b) => b.month.localeCompare(a.month)); // Sort months descending

  } catch (e) {
    console.error('Failed to fetch stats:', e);
  } finally {
    loading.value = false;
  }
}

async function viewAlphaList(month, region) {
  selectedMonth.value = month;
  selectedRegion.value = region || '';
  showAlphaList.value = true;
  alphaLoading.value = true;
  alphaListData.value = [];

  try {
    // Build query to match month and optionally region
    const conditions = [
      {
        $or: [
          { "dateSubmitted": { $regex: `^${month}` } },
          { $and: [{ "dateSubmitted": { $exists: false } }, { "dateCreated": { $regex: `^${month}` } }] }
        ]
      }
    ];
    
    if (region) {
      conditions.push({ "settings.region": region });
    }

    const query = { $and: conditions };

    const params = {
      collection: selectedCollection.value,
      query: JSON.stringify(query),
      page: 1,
      page_size: 1000
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