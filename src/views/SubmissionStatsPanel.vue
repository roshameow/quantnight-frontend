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
        
        <div v-if="uniqueMonths.length > 0" style="width: 300px; margin-left: 24px">
          <n-space vertical size="small">
            <n-text depth="3" style="font-size: 12px">
              3个月统计窗口: 
              <n-tag v-for="m in selectedMonthsNames" :key="m" size="tiny" style="margin-right: 4px" type="success">
                {{ m }}
              </n-tag>
            </n-text>
            <n-slider
              v-model:value="windowStartIndex"
              :max="maxWindowIndex"
              :step="1"
              :tooltip="false"
            />
          </n-space>
        </div>
      </n-space>
    </div>

    <n-data-table
      :columns="columns"
      :data="combinedStatsData"
      :loading="loading"
      :bordered="false"
      size="small"
      class="stats-table"
      :row-key="(row) => row.key"
      :row-class-name="rowClassName"
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
import { ref, onMounted, h, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { NButton, NSpace, NSelect, NDataTable, NModal, NSlider, NText, NTag } from 'naive-ui';
import { useConfigStore } from '../stores/configStore';
import { useAlphaTableColumns } from '../composables/useAlphaTableColumns';

const configStore = useConfigStore();
const loading = ref(false);
const selectedCollection = ref('alpha_results');
const statsData = ref([]);
const rawStatsResult = ref([]);

const windowStartIndex = ref(0);

const uniqueMonths = computed(() => {
  const months = statsData.value.map(m => m.month);
  return months;
});

const maxWindowIndex = computed(() => {
  return Math.max(0, uniqueMonths.value.length - 3);
});

const selectedMonthsNames = computed(() => {
  const start = windowStartIndex.value;
  const end = Math.min(start + 3, uniqueMonths.value.length);
  return uniqueMonths.value.slice(start, end);
});

const rowClassName = (row) => {
  if (row.month === '全部时间' || row.month === '选定3个月') {
    return 'summary-row';
  }
  if (row.is_summary_child) {
    return 'summary-child-row';
  }
  const windowNames = selectedMonthsNames.value;
  if (windowNames.includes(row.month)) {
    let classes = 'selected-month-row';
    // Mark the start and end of the window for specific borders
    // Since it's sorted descending, windowNames[0] is the top row
    if (row.month === windowNames[0] && !row.region) {
      classes += ' window-start';
    }
    if (row.month === windowNames[windowNames.length - 1] && !row.region) {
      classes += ' window-end';
    }
    return classes;
  }
  return '';
};

function calculateGroupedSummary(docs, label) {
  if (!docs || docs.length === 0) return null;
  
  const regionGroups = {};
  let totalCount = 0;
  let weightedSharpe = 0;
  let weightedFitness = 0;
  let weightedTurnover = 0;
  let weightedReturns = 0;
  let weightedMargin = 0;
  
  docs.forEach(doc => {
    const region = doc._id.region || 'Unknown';
    if (!regionGroups[region]) {
      regionGroups[region] = {
        region,
        count: 0,
        avg_sharpe: 0,
        avg_fitness: 0,
        avg_turnover: 0,
        avg_returns: 0,
        avg_margin: 0
      };
    }
    
    const r = regionGroups[region];
    r.count += doc.count;
    r.avg_sharpe += (doc.avg_sharpe || 0) * doc.count;
    r.avg_fitness += (doc.avg_fitness || 0) * doc.count;
    r.avg_turnover += (doc.avg_turnover || 0) * doc.count;
    r.avg_returns += (doc.avg_returns || 0) * doc.count;
    r.avg_margin += (doc.avg_margin || 0) * doc.count;
    
    totalCount += doc.count;
    weightedSharpe += (doc.avg_sharpe || 0) * doc.count;
    weightedFitness += (doc.avg_fitness || 0) * doc.count;
    weightedTurnover += (doc.avg_turnover || 0) * doc.count;
    weightedReturns += (doc.avg_returns || 0) * doc.count;
    weightedMargin += (doc.avg_margin || 0) * doc.count;
  });

  const children = Object.values(regionGroups).map(r => {
    if (r.count > 0) {
      r.avg_sharpe /= r.count;
      r.avg_fitness /= r.count;
      r.avg_turnover /= r.count;
      r.avg_returns /= r.count;
      r.avg_margin /= r.count;
    }
    return {
      ...r,
      key: `${label}-${r.region}`,
      month: label,
      is_summary_child: true
    };
  }).sort((a, b) => a.region.localeCompare(b.region));

  if (totalCount === 0) return null;

  return {
    key: label,
    month: label,
    count: totalCount,
    avg_sharpe: weightedSharpe / totalCount,
    avg_fitness: weightedFitness / totalCount,
    avg_turnover: weightedTurnover / totalCount,
    avg_returns: weightedReturns / totalCount,
    avg_margin: weightedMargin / totalCount,
    children
  };
}

const totalStatsRow = computed(() => {
  return calculateGroupedSummary(rawStatsResult.value, '全部时间');
});

const windowStatsRow = computed(() => {
  const filteredDocs = rawStatsResult.value.filter(doc => 
    selectedMonthsNames.value.includes(doc._id.month)
  );
  return calculateGroupedSummary(filteredDocs, '选定3个月');
});

const combinedStatsData = computed(() => {
  const result = [];
  if (totalStatsRow.value) result.push(totalStatsRow.value);
  if (windowStatsRow.value) result.push(windowStatsRow.value);
  return [...result, ...statsData.value];
});

const showAlphaList = ref(false);
const selectedMonth = ref('');
const selectedRegion = ref('');
const alphaListData = ref([]);
const alphaLoading = ref(false);

const pnlDataMap = ref({});
const loadingSet = ref(new Set());
const expandedRowIds = ref(new Set());

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
    
    rawStatsResult.value = result;
    
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
      
      monthGroups[month].count += doc.count;
      monthGroups[month].avg_sharpe += (doc.avg_sharpe || 0) * doc.count;
      monthGroups[month].avg_fitness += (doc.avg_fitness || 0) * doc.count;
      monthGroups[month].avg_turnover += (doc.avg_turnover || 0) * doc.count;
      monthGroups[month].avg_returns += (doc.avg_returns || 0) * doc.count;
      monthGroups[month].avg_margin += (doc.avg_margin || 0) * doc.count;
    });

    statsData.value = Object.values(monthGroups).map(m => {
      if (m.count > 0) {
        m.avg_sharpe /= m.count;
        m.avg_fitness /= m.count;
        m.avg_turnover /= m.count;
        m.avg_returns /= m.count;
        m.avg_margin /= m.count;
      }
      m.children.sort((a, b) => a.region.localeCompare(b.region));
      return {
        ...m,
        key: m.month
      };
    }).sort((a, b) => b.month.localeCompare(a.month));

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
    let monthCondition = {};
    if (month === '全部时间') {
      monthCondition = {};
    } else if (month === '选定3个月') {
      monthCondition = {
        $or: selectedMonthsNames.value.map(m => ({
          $or: [
            { "dateSubmitted": { $regex: `^${m}` } },
            { $and: [{ "dateSubmitted": { $exists: false } }, { "dateCreated": { $regex: `^${m}` } }] }
          ]
        }))
      };
    } else {
      monthCondition = {
        $or: [
          { "dateSubmitted": { $regex: `^${month}` } },
          { $and: [{ "dateSubmitted": { $exists: false } }, { "dateCreated": { $regex: `^${month}` } }] }
        ]
      };
    }

    const conditions = [];
    if (Object.keys(monthCondition).length > 0) {
      conditions.push(monthCondition);
    }
    if (region) {
      conditions.push({ "settings.region": region });
    }

    const query = conditions.length > 0 ? { $and: conditions } : {};

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
:deep(.selected-month-row) {
  background-color: rgba(24, 160, 88, 0.08) !important;
}

:deep(.selected-month-row td:first-child) {
  border-left: 2px solid #18a058 !important;
}

:deep(.selected-month-row td:last-child) {
  border-right: 2px solid #18a058 !important;
}

:deep(.selected-month-row.window-start td) {
  border-top: 2px solid #18a058 !important;
}

:deep(.selected-month-row.window-end td) {
  border-bottom: 2px solid #18a058 !important;
}

:deep(.summary-row) {
  background-color: rgba(24, 160, 88, 0.15) !important;
  font-weight: bold;
}
:deep(.summary-child-row) {
  background-color: rgba(24, 160, 88, 0.03) !important;
}
</style>