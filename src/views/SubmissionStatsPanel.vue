<template>
  <div class="submission-stats-container">
    <div class="header">
      <div style="background: #f8f8f8; padding: 16px; border-radius: 8px; margin-bottom: 16px">
        <div style="display: flex; align-items: center; gap: 12px;">
          <!-- 数据集选择 -->
          <div style="width: 160px; flex-shrink: 0;">
            <n-select
              v-model:value="selectedCollection"
              :options="configStore.dataFilterOptions"
              placeholder="选择数据集"
              style="width: 100%"
              @update:value="fetchStats"
            />
          </div>

          <!-- 查询输入 -->
          <div style="flex: 1; min-width: 200px;">
            <n-auto-complete
              v-model:value="searchQuery"
              :options="historyOptions"
              :render-label="renderLabel"
              placeholder="输入查询条件 (JSON, 回车搜索)"
              clearable
              @select="handleHistorySelect"
              @keyup.enter="fetchStats"
            />
          </div>

          <!-- 搜索按钮 -->
          <div style="width: 100px; flex-shrink: 0;">
            <n-button type="primary" @click="fetchStats" :loading="loading" style="width: 100%">
              刷新统计
            </n-button>
          </div>
          
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
        </div>
      </div>
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
      :title="`Alpha 列表 - ${selectedMonth} ${selectedRegion ? '- ' + selectedRegion : ''}${selectedDelay !== null ? ' - Delay ' + selectedDelay : ''}`"
      style="width: 90%; max-width: 1200px"
    >
      <div style="height: 600px; overflow: auto">
        <n-data-table
          :columns="alphaColumns"
          :data="alphaListData"
          :loading="alphaLoading"
          :bordered="false"
          size="small"
          class="custom-table"
          :scroll-x="1200"
          :row-key="(row) => row.id"
        />
      </div>
    </n-modal>
  </div>
</template>

<script setup>
import { ref, onMounted, h, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { NButton, NSpace, NSelect, NDataTable, NModal, NSlider, NText, NTag, NAutoComplete } from 'naive-ui';
import { useConfigStore } from '../stores/configStore';
import { useQueryStore } from '../stores/queryStore';
import { useAlphaTableColumns } from '../composables/useAlphaTableColumns';
import '../assets/table-styles.css';

const configStore = useConfigStore();
const queryStore = useQueryStore();

const loading = ref(false);
const selectedCollection = ref('alpha_results');
const searchQuery = ref('');
const statsData = ref([]);
const rawStatsResult = ref([]);

const windowStartIndex = ref(0);

// --- Query History Logic ---
const historyOptions = computed(() => {
  const queries = queryStore.getQueriesByType('submission');
  if (queries.length === 0) {
    return [{ label: '暂无历史记录', value: '', disabled: true }];
  }
  return queries
    .filter(q => q.label && q.label !== 'All Data') 
    .map(q => ({
      label: q.label,
      value: q.label,
      id: q.id
    }));
});

const handleHistorySelect = (value) => {
  if (!value) return;
  searchQuery.value = value;
  fetchStats();
};

const removeQuery = (id) => {
  queryStore.removeQuery(id);
};

const renderLabel = (option) => {
  if (option.disabled) return option.label;
  
  return h('div', { 
    style: 'display: flex; align-items: flex-start; justify-content: space-between; width: 100%; min-width: 300px; padding: 4px 0;' 
  }, [
    h('span', { 
      style: 'flex: 1; white-space: normal; word-break: break-all; line-height: 1.4; padding-right: 8px;' 
    }, option.label),
    h(NButton, {
      quaternary: true,
      circle: true,
      size: 'tiny',
      type: 'error',
      style: 'flex-shrink: 0;',
      onClick: (e) => {
        e.stopPropagation();
        removeQuery(option.id);
      }
    }, {
      default: () => '×'
    })
  ]);
};

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
  
  const groups = {}; // Key will be region-delay
  let totalCount = 0;
  let totalSuperCount = 0;
  let weightedSharpe = 0;
  let weightedFitness = 0;
  let weightedTurnover = 0;
  let weightedReturns = 0;
  let weightedMargin = 0;
  
  docs.forEach(doc => {
    const region = doc._id.region || 'Unknown';
    const delay = doc._id.delay || 0;
    const groupKey = `${region}-${delay}`;
    
    if (!groups[groupKey]) {
      groups[groupKey] = {
        region,
        delay,
        count: 0,
        super_count: 0,
        avg_sharpe: 0,
        avg_fitness: 0,
        avg_turnover: 0,
        avg_returns: 0,
        avg_margin: 0
      };
    }
    
    const g = groups[groupKey];
    const docCount = doc.count || 0;
    const docSuperCount = doc.super_count || 0;

    g.count += docCount;
    g.super_count += docSuperCount;
    g.avg_sharpe += (doc.avg_sharpe || 0) * docCount;
    g.avg_fitness += (doc.avg_fitness || 0) * docCount;
    g.avg_turnover += (doc.avg_turnover || 0) * docCount;
    g.avg_returns += (doc.avg_returns || 0) * docCount;
    g.avg_margin += (doc.avg_margin || 0) * docCount;
    
    totalCount += docCount;
    totalSuperCount += docSuperCount;
    weightedSharpe += (doc.avg_sharpe || 0) * docCount;
    weightedFitness += (doc.avg_fitness || 0) * docCount;
    weightedTurnover += (doc.avg_turnover || 0) * docCount;
    weightedReturns += (doc.avg_returns || 0) * docCount;
    weightedMargin += (doc.avg_margin || 0) * docCount;
  });

  const children = Object.values(groups).map(g => {
    if (g.count > 0) {
      g.avg_sharpe /= g.count;
      g.avg_fitness /= g.count;
      g.avg_turnover /= g.count;
      g.avg_returns /= g.count;
      g.avg_margin /= g.count;
    }
    return {
      ...g,
      key: `${label}-${g.region}-${g.delay}`,
      month: label,
      is_summary_child: true
    };
  }).sort((a, b) => {
    const regionComp = a.region.localeCompare(b.region);
    if (regionComp !== 0) return regionComp;
    return a.delay - b.delay;
  });

  if (totalCount === 0 && totalSuperCount === 0) return null;

  return {
    key: label,
    month: label,
    count: totalCount,
    super_count: totalSuperCount,
    avg_sharpe: totalCount > 0 ? weightedSharpe / totalCount : 0,
    avg_fitness: totalCount > 0 ? weightedFitness / totalCount : 0,
    avg_turnover: totalCount > 0 ? weightedTurnover / totalCount : 0,
    avg_returns: totalCount > 0 ? weightedReturns / totalCount : 0,
    avg_margin: totalCount > 0 ? weightedMargin / totalCount : 0,
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
const selectedDelay = ref(null);
const alphaListData = ref([]);
const alphaLoading = ref(false);

const pnlDataMap = ref({});
const loadingSet = ref(new Set());
const expandedRowIds = ref(new Set());
const visibleColumns = ref(['score', 'universe', 'pnl', 'message']);

const { columns: alphaColumns } = useAlphaTableColumns({
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
  },
  visibleColumns
});

const columns = [
  {
    title: '时间 / 区域 / Delay',
    key: 'label',
    render(row) {
      if (row.region) {
        return h('span', [
          h('span', row.region),
          '-',
          h('span', { style: 'color: #999' }, row.delay)
        ]);
      }
      return row.month;
    }
  },
  {
    title: '提交个数',
    key: 'count',
    render(row) {
      return h('div', { style: 'display: flex; align-items: center; gap: 12px' }, [
        // Regular count: fixed width, right aligned
        h('div', { style: 'width: 40px; display: flex; justify-content: flex-end' }, [
          h(
            NButton,
            {
              text: true,
              type: 'primary',
              onClick: () => viewAlphaList(row.month, row.region, row.delay, 'REGULAR')
            },
            { default: () => row.count }
          )
        ]),
        // Super count: fixed width, right aligned, plain blue text
        h('div', { style: 'width: 30px; display: flex; justify-content: flex-end' }, [
          row.super_count > 0 ? h(
            NButton,
            {
              text: true,
              style: 'color: #2080f0',
              onClick: () => viewAlphaList(row.month, row.region, row.delay, 'SUPER')
            },
            { default: () => row.super_count }
          ) : null
        ])
      ]);
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
  const q = searchQuery.value.trim();
  if (q && q !== '{}') {
    queryStore.addQuery('submission', q);
  }

  try {
    const result = await invoke('get_submission_stats', {
      collection: selectedCollection.value,
      query: q || null
    });
    
    rawStatsResult.value = result;
    
    const monthGroups = {};
    result.forEach(doc => {
      const month = doc._id.month || 'Unknown';
      if (!monthGroups[month]) {
        monthGroups[month] = {
          month,
          count: 0,
          super_count: 0,
          avg_sharpe: 0,
          avg_fitness: 0,
          avg_turnover: 0,
          avg_returns: 0,
          avg_margin: 0,
          children: []
        };
      }
      
      const regionData = {
        key: `${month}-${doc._id.region}-${doc._id.delay}`,
        month,
        region: doc._id.region,
        delay: doc._id.delay,
        count: doc.count || 0,
        super_count: doc.super_count || 0,
        avg_sharpe: doc.avg_sharpe,
        avg_fitness: doc.avg_fitness,
        avg_turnover: doc.avg_turnover,
        avg_returns: doc.avg_returns,
        avg_margin: doc.avg_margin
      };
      
      monthGroups[month].children.push(regionData);
      
      const docCount = doc.count || 0;
      monthGroups[month].count += docCount;
      monthGroups[month].super_count += (doc.super_count || 0);
      monthGroups[month].avg_sharpe += (doc.avg_sharpe || 0) * docCount;
      monthGroups[month].avg_fitness += (doc.avg_fitness || 0) * docCount;
      monthGroups[month].avg_turnover += (doc.avg_turnover || 0) * docCount;
      monthGroups[month].avg_returns += (doc.avg_returns || 0) * docCount;
      monthGroups[month].avg_margin += (doc.avg_margin || 0) * docCount;
    });

    statsData.value = Object.values(monthGroups).map(m => {
      if (m.count > 0) {
        m.avg_sharpe /= m.count;
        m.avg_fitness /= m.count;
        m.avg_turnover /= m.count;
        m.avg_returns /= m.count;
        m.avg_margin /= m.count;
      }
      m.children.sort((a, b) => {
        const regionComp = a.region.localeCompare(b.region);
        if (regionComp !== 0) return regionComp;
        return a.delay - b.delay;
      });
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

async function viewAlphaList(month, region, delay, alphaType) {
  selectedMonth.value = month;
  selectedRegion.value = region || '';
  selectedDelay.value = (delay !== undefined && delay !== null) ? delay : null;
  if (alphaType) selectedRegion.value += ` (${alphaType})`;

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
    if (delay !== undefined && delay !== null) {
      conditions.push({ "settings.delay": delay });
    }
    if (alphaType) {
      conditions.push({ "type": alphaType });
    }

    // Add search query conditions if present
    const q = searchQuery.value.trim();
    if (q && q !== '{}') {
      try {
        conditions.push(JSON.parse(q));
      } catch (e) {
        console.error("Failed to parse search query JSON:", e);
      }
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