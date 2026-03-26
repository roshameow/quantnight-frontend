<template>
  <div>
    <!-- 数据集和查询选择区域 -->
    <div style="background: #f8f8f8; padding: 16px; border-radius: 8px; margin-bottom: 16px">
      <div style="display: flex; align-items: center; gap: 12px;">
        <!-- 数据集选择 -->
        <div style="width: 160px; flex-shrink: 0;">
          <n-select
            v-model:value="selectedCollection"
            :options="configStore.dataFilterOptions"
            placeholder="选择数据集"
            style="width: 100%"
          />
        </div>

        <!-- 查询输入 - 自动占据剩余空间 -->
        <div style="flex: 1; min-width: 200px;">
          <n-auto-complete
            v-model:value="searchQuery"
            :options="historyOptions"
            :render-label="renderLabel"
            placeholder="输入查询条件（回车搜索）"
            clearable
            @select="handleHistorySelect"
            @keyup.enter="() => searchAlphas()"
          />
        </div>

        <!-- Region输入 -->
        <div style="width: 100px; flex-shrink: 0;">
          <n-input
            v-model:value="region"
            placeholder="Region"
            clearable
            style="width: 100%"
            @keyup.enter="() => searchAlphas()"
          />
        </div>

        <!-- Delay输入 -->
        <div style="width: 80px; flex-shrink: 0;">
          <n-input
            v-model:value="delay"
            placeholder="Delay"
            clearable
            style="width: 100%"
            @keyup.enter="() => searchAlphas()"
          />
        </div>

        <!-- 搜索按钮 -->
        <div style="width: 100px; flex-shrink: 0;">
          <n-button type="primary" @click="() => searchAlphas()" :loading="searchLoading" style="width: 100%">
            搜索
          </n-button>
        </div>
      </div>
    </div>

    <!-- 主内容区域：Alpha选择和PNL图表横向并列 -->
    <div style="display: flex; gap: 16px; background: #f8f8f8; padding: 16px; border-radius: 8px">
      <!-- Alpha选择区域 -->
      <div style="width: 120px; flex-shrink: 0">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px">
          <h3 style="margin: 0; font-size: 14px">Alpha选择 ({{ visibleSelectedCount }}/{{ searchResults.length }})</h3>
          <n-space size="small" vertical>
            <n-button size="tiny" @click="exportSelection" :disabled="selectedAlphaIds.size === 0" style="width: 100%">
              导出选中
            </n-button>
            <n-button size="tiny" @click="importSelection" style="width: 100%">
              导入列表
            </n-button>
          </n-space>
        </div>
        
        <div v-if="searchResults.length === 0 && !searchLoading && selectedAlphaIds.size === 0" style="color: #999; text-align: center; padding: 10px; font-size: 12px">
          暂无搜索结果
        </div>
        
        <div>
          <!-- 列表框容器 -->
          <div style="position: relative; border: 1px solid #e0e0e0; border-radius: 4px">
            <!-- 滚动区域 -->
            <div style="height: 400px; overflow-y: auto">
              <div
                v-for="alpha in sortedAlphaList"
                :key="alpha.id"
                @click="toggleAlphaSelection(alpha.id)"
                :style="{
                  cursor: 'pointer',
                  padding: '2px 6px',
                  fontSize: '11px',
                  backgroundColor: selectedAlphaIds.has(alpha.id) ? getColorForAlpha(alpha.id, true) + '20' : 'transparent',
                  borderBottom: '1px solid #f0f0f0'
                }"
              >
                <div style="display: flex; align-items: center; justify-content: space-between">
                  <span style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">{{ alpha.id }}</span>
                  <div style="display: flex; align-items: center; flex-shrink: 0;">
                    <n-checkbox
                      :checked="selectedAlphaIds.has(alpha.id)"
                      @update:checked="toggleAlphaSelection(alpha.id)"
                      @click.stop
                      size="small"
                    />
                    <n-button text size="tiny" @click.stop="removeAlpha(alpha.id)" style="margin-left: 4px; padding: 0 4px; font-weight: bold;">
                      ×
                    </n-button>
                  </div>
                </div>
              </div>
              
              <!-- 添加新Alpha的可编辑行 -->
              <div style="padding: 2px 6px; fontSize: 11px; borderBottom: 1px solid #f0f0f0">
                <n-input
                  v-model:value="newAlphaId"
                  placeholder="输入新Alpha ID"
                  size="tiny"
                  style="width: 100%"
                 
                  @keyup.enter="addAlpha"
                />
              </div>
            </div>
            
            <!-- + 和 - 按钮固定在右下角 -->
            <div style="position: absolute; bottom: 0; right: 0; display: flex; gap: 0px">
              <n-button size="tiny" @click="addAlpha" :disabled="!newAlphaId.trim()" style="min-width: 24px; border-top-right-radius: 0; border-bottom-right-radius: 0; background-color: white; opacity: 1">
                +
              </n-button>
              <n-button size="tiny" @click="removeSelectedAlpha" :disabled="selectedAlphaIds.size === 0" style="min-width: 24px; border-top-left-radius: 0; border-bottom-left-radius: 0; margin-left: -1px; background-color: white; opacity: 1">
                -
              </n-button>
            </div>
          </div>
        </div>
        
        <!-- 动态调整按钮 -->
        <div style="margin-top: 8px">
          <div style="display: flex; gap: 8px; margin-bottom: 8px;">
            <n-button size="tiny" @click="selectAll" :disabled="searchResults.length === 0" style="flex: 1">
              全选
            </n-button>
            <n-button size="tiny" @click="deselectAll" :disabled="selectedAlphaIds.size === 0" style="flex: 1">
              全不选
            </n-button>
          </div>
          <n-button size="tiny" type="error" @click="clearAlphaList" :disabled="searchResults.length === 0 && selectedAlphaIds.size === 0" style="width: 100%">
            清空列表
          </n-button>
        </div>
      </div>

      <!-- Charts Area: PnL and PCA side-by-side -->
      <div style="flex: 1; min-width: 0;">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; gap: 8px;">
          <h3 style="margin: 0; white-space: nowrap;">数据可视化</h3>

        </div>
        
        <div style="display: flex; gap: 16px; min-height: 500px;">
          <div v-if="searchResults.length === 0 && selectedAlphaIds.size === 0" style="color: #999; text-align: center; padding: 40px; width: 100%;">
            请先搜索Alpha以显示PNL图表
          </div>
          <div v-else :style="{width: clusterAlphas.length > 0 ? '50%' : '100%', display: 'flex', 'flex-direction': 'column'}">
            <div style="display: flex; flex-direction: column; align-items: flex-start; gap: 4px; margin-bottom: 8px;">
              <div style="display: flex; justify-content: space-between; align-items: center; width: 100%; padding-left: 50px; padding-right: 60px; box-sizing: border-box;">
                <h4 style="margin: 0;">PnL 对比</h4>
                <n-text v-if="!showSelectedOnly && searchResults.length > 30" type="warning" style="font-size: 12px; white-space: nowrap; margin-left: auto;">
                    (PnL未选中仅显示前30)
                </n-text>
                <n-switch v-model:value="showSelectedOnly" style="margin-left: 8px;">
                  <template #checked>
                    仅显示选中(PnL)
                  </template>
                  <template #unchecked>
                    显示全部(PnL)
                  </template>
                </n-switch>
                <n-switch v-model:value="showAveragePnL" style="margin-left: 8px;">
                  <template #checked>
                    显示平均PnL
                  </template>
                  <template #unchecked>
                    不显示平均PnL
                  </template>
                </n-switch>
              </div>
            </div>
            <v-chart
              ref="chartRef"
              :option="chartOption"
              style="width: 100%; flex: 1"
              @click="handleChartClick"
              @datazoom="handleDataZoom"
              :autoresize="true"
            />
          </div>
          <div v-if="clusterAlphas.length > 0" style="width: 50%; display: flex; flex-direction: column;">
            <div style="display: flex; flex-direction: column; align-items: flex-start; gap: 4px; margin-bottom: 8px;">
              <h4 style="margin: 0;">聚类分析</h4>
              <n-select
                v-model:value="embedding"
                :options="configStore.embeddingOptions"
                placeholder="Embedding"
                size="tiny"
                style="width: 160px"
                clearable
              />
            </div>
            <div style="position: relative; flex: 1;">
              <v-chart
                ref="clusterChartRef"
                :option="clusterChartOption"
                style="width: 100%; height: 100%; box-sizing: border-box;"
                :autoresize="true"
                @click="handleClusterChartClick"
                @datazoom="handleClusterDataZoom"
              />
              <n-button-group size="tiny" style="position: absolute; top: 10px; left: 10px; z-index: 10;">
                <n-button @click="zoomInCluster"> + </n-button>
                <n-button @click="zoomOutCluster"> - </n-button>
              </n-button-group>

              <!-- Custom Overlay Legend -->
              <div v-if="isLegendCollapsed"
                   style="position: absolute; top: 10px; right: 10px; z-index: 10; cursor: pointer; display: flex; align-items: center; justify-content: flex-end; gap: 4px; padding: 2px 6px;"
                   @click="isLegendCollapsed = false">
                <span style="font-size: 13px; color: #333;">Clusters</span>
                <n-button text size="tiny" style="font-size: 12px; color: #333;">
                  ◀
                </n-button>
              </div>

              <div v-else
                   style="position: absolute; top: 10px; right: 10px; width: 200px; max-height: 80%; z-index: 10; background-color: rgba(255, 255, 255, 0.8); border-radius: 8px; padding: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.15); display: flex; flex-direction: column;">
                  <div style="display: flex; justify-content: flex-end; align-items: center; cursor: pointer; margin-bottom: 4px; padding: 0 2px;" @click="isLegendCollapsed = true">
                    <n-button text size="tiny" style="font-size: 12px;">
                      ▲
                    </n-button>
                  </div>
                  <div style="flex: 1; overflow-y: auto;">
                    <div 
                      v-for="item in clusterLegendData" 
                      :key="item.id"
                      @mouseover="highlightCluster(item.name)"
                      @mouseout="downplayCluster(item.name)"
                      @click="selectAlphasInCluster(item.id)"
                      style="display: flex; align-items: center; gap: 8px; margin-bottom: 4px; cursor: pointer; padding: 2px; border-radius: 4px;"
                      class="legend-item"
                    >
                      <span :style="{ backgroundColor: item.color, width: '12px', height: '12px', borderRadius: '50%', display: 'inline-block' }"></span>
                      <span style="font-size: 12px; flex: 1; word-break: break-all;">{{ item.id }} ({{ item.selectedCount }}/{{ item.count }})</span>
                    </div>
                  </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Selected Alphas Table -->
    <div v-if="selectedAlphaIds.size > 0" style="margin-top: 16px; background: #fff; padding: 16px; border-radius: 8px; border: 1px solid #eee;">
      <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px">
        <h3 style="margin: 0">选中的 Alpha 详情</h3>
        <div style="display: flex; align-items: center; gap: 12px; background: #f8f8f8; padding: 4px 12px; border-radius: 4px;">
          <span style="font-size: 13px; color: #666;">显示详情:</span>
          <n-checkbox-group v-model:value="visibleColumns">
            <n-space>
              <n-checkbox v-for="opt in columnOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </n-checkbox>
            </n-space>
          </n-checkbox-group>
        </div>
      </div>
      <n-data-table
        :columns="filteredColumns"
        :data="sortedAlphaDetails"
        :bordered="false"
        :scroll-x="1200"
        class="custom-table"
        :row-key="(row) => row.id"
        @update:sorter="handleSorterChange"
      />
    </div>
  </div>
</template>

<script setup>
import { ref, computed, watch, onMounted, nextTick, markRaw, h } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useConfigStore } from "../stores/configStore";
import { useAnalysisStore } from "../stores/analysisStore";
import { useQueryStore } from "../stores/queryStore";
import { storeToRefs } from "pinia";
import VChart from "vue-echarts";
import { use } from "echarts/core";
import { LineChart, ScatterChart } from "echarts/charts";
import { GridComponent, TooltipComponent, LegendComponent, DataZoomComponent, VisualMapComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import { useMessage, useDialog, NButton } from 'naive-ui';
import { useAlphaTableColumns } from "../composables/useAlphaTableColumns.js";
import "../assets/table-styles.css";

// Register ECharts components
use([LineChart, ScatterChart, GridComponent, TooltipComponent, LegendComponent, DataZoomComponent, VisualMapComponent, CanvasRenderer]);

// --- State ---
const configStore = useConfigStore();
const analysisStore = useAnalysisStore();
const queryStore = useQueryStore();
const {
  selectedCollection,
  embedding,
  searchQuery,
  region,
  delay,
  searchResults,
  selectedAlphaIds,
  selectedAlphasMap,
  pnlDataMap,
  showSelectedOnly,
  showAveragePnL,
  chartZoomState
} = storeToRefs(analysisStore);

const averageMetrics = ref({ sharpe: 0, returns: 0, turnover: 0, margin: 0, fitness: 0, drawdown: 0 });
const message = useMessage();
const dialog = useDialog();
const searchLoading = ref(false);
const newQueryName = ref("");
const newAlphaId = ref("");

const historyOptions = computed(() => {
  const queries = queryStore.getQueriesByType('analysis');
  if (queries.length === 0) {
    return [{ label: '暂无历史记录', value: '', disabled: true }];
  }
  // 只返回有意义的查询字符串，并确保 value 是字符串
  return queries
    .filter(q => q.label && q.label !== 'All Data') 
    .map(q => ({
      label: q.label,
      value: q.label, // n-auto-complete 选中后会将此值填入 input
      id: q.id        // 保留 ID 用于删除
    }));
});

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
        // 通过 option 中保留的 id 进行删除
        queryStore.removeQuery(option.id);
        message.success('已删除历史查询');
      }
    }, {
      default: () => '×'
    })
  ]);
};

const handleHistorySelect = (value) => {
  if (!value) return;
  // 直接赋值，n-auto-complete 会自动更新绑定的 searchQuery
  searchQuery.value = value;
  message.success(`已加载历史查询内容`);
};
const loadingSet = ref(new Set());
const chartRef = ref(null);
const clusterChartRef = ref(null);
const sortKey = ref(null);
const sortOrder = ref(null);
const isLegendCollapsed = ref(false); // 控制聚类图例的折叠状态
const clusterChartZoomState = ref({
  xAxisStart: 0,
  xAxisEnd: 100,
  yAxisStart: 0,
  yAxisEnd: 100,
});

const visibleColumns = ref(['score', 'pnl', 'message', 'corr']);

const columnOptions = [
  { label: 'Score', value: 'score' },
  { label: 'OS Stats', value: 'os' },
  { label: 'Universe', value: 'universe' },
  { label: 'Neutralization', value: 'neutralization' },
  { label: 'Drawdown', value: 'drawdown' },
  { label: 'PnL', value: 'pnl' },
  { label: 'Message', value: 'message' },
  { label: 'Correlation', value: 'corr' },
];

// --- Chart Interactivity ---
function highlightCluster(seriesName) {
  clusterChartRef.value?.dispatchAction({
    type: 'highlight',
    seriesName: seriesName
  });
}

function downplayCluster(seriesName) {
  clusterChartRef.value?.dispatchAction({
    type: 'downplay',
    seriesName: seriesName
  });
}

function selectAlphasInCluster(clusterId) {
  const alphasInCluster = searchResults.value.filter(a => a.cluster_id === clusterId);
  if (alphasInCluster.length === 0) return;

  const alphaIdsInCluster = alphasInCluster.map(a => a.id);

  // Check if all alphas in the cluster are already selected
  const allSelected = alphaIdsInCluster.every(id => selectedAlphaIds.value.has(id));

  const newIdSet = new Set(selectedAlphaIds.value);
  const newMap = new Map(selectedAlphasMap.value);

  if (allSelected) {
    // If all are selected, deselect them
    alphaIdsInCluster.forEach(id => {
      newIdSet.delete(id);
      newMap.delete(id);
    });
    message.info(`已取消选择 Cluster ${clusterId} 中的 ${alphaIdsInCluster.length} 个 Alpha`);
  } else {
    // If some or none are selected, select all of them
    alphasInCluster.forEach(alpha => {
      newIdSet.add(alpha.id);
      newMap.set(alpha.id, alpha);
    });
    message.success(`已选择 Cluster ${clusterId} 中的 ${alphasInCluster.length} 个 Alpha`);
  }

  selectedAlphaIds.value = newIdSet;
  selectedAlphasMap.value = newMap;
}


// --- Zoom Methods ---
function zoomCluster(zoomFactor) {
  const { xAxisStart, xAxisEnd, yAxisStart, yAxisEnd } = clusterChartZoomState.value;

  const xRange = xAxisEnd - xAxisStart;
  const yRange = yAxisEnd - yAxisStart;

  const newXRange = xRange * zoomFactor;
  const newYRange = yRange * zoomFactor;

  const xCenter = (xAxisStart + xAxisEnd) / 2;
  const yCenter = (yAxisStart + yAxisEnd) / 2;

  let newXStart = xCenter - newXRange / 2;
  let newXEnd = xCenter + newXRange / 2;
  let newYStart = yCenter - newYRange / 2;
  let newYEnd = yCenter + newYRange / 2;

  // Clamp values to the [0, 100] range
  newXStart = Math.max(0, newXStart);
  newXEnd = Math.min(100, newXEnd);
  newYStart = Math.max(0, newYStart);
  newYEnd = Math.min(100, newYEnd);
  
  // Prevent getting stuck if zoomed in too far
  if (newXEnd - newXStart < 0.01) newXEnd = newXStart + 0.01;
  if (newYEnd - newYStart < 0.01) newYEnd = newYStart + 0.01;

  // Dispatch actions directly to ECharts
  clusterChartRef.value?.dispatchAction({
    type: 'dataZoom',
    dataZoomId: 'dataZoomX',
    start: newXStart,
    end: newXEnd
  });
   clusterChartRef.value?.dispatchAction({
    type: 'dataZoom',
    dataZoomId: 'dataZoomY',
    start: newYStart,
    end: newYEnd
  });
  // The @datazoom event will update the clusterChartZoomState ref
}

function zoomInCluster() {
  zoomCluster(0.8);
}

function zoomOutCluster() {
  zoomCluster(1.25);
}

function handleClusterDataZoom(params) {
  // This function is called on pan/drag, so we need to update our state.
  const updateZoomState = (payload) => {
    if (payload.dataZoomId === 'dataZoomX') {
      clusterChartZoomState.value.xAxisStart = payload.start;
      clusterChartZoomState.value.xAxisEnd = payload.end;
    }
    if (payload.dataZoomId === 'dataZoomY') {
      clusterChartZoomState.value.yAxisStart = payload.start;
      clusterChartZoomState.value.yAxisEnd = payload.end;
    }
  };

  if (params.batch) {
    // Inside datazoom events often come in a batch
    params.batch.forEach(updateZoomState);
  } else {
    updateZoomState(params);
  }
}

// --- Table Setup ---
const expandedRowIds = ref(new Set());
const { columns } = useAlphaTableColumns({
  expandedRowIds,
  pnlDataMap,
  loadingSet,
  loadPNL,
  visibleColumns,
});

const filteredColumns = computed(() => {
  const excludedKeys = ['message', 'pnl', 'corr_ppac', 'corr_os'];
  return columns.value.filter(col => !excludedKeys.includes(col.key));
});

const selectedAlphaDetails = computed(() => {
  return Array.from(selectedAlphasMap.value.values());
});

const sortedAlphaDetails = computed(() => {
  let data = selectedAlphaDetails.value;
  const key = sortKey.value;
  const order = sortOrder.value;

  let sortedData = [...data];
  if (key && order) {
    sortedData.sort((a, b) => {
      const valA = a[key];
      const valB = b[key];
      const orderFactor = order === 'ascend' ? 1 : -1;

      if (valA === null || valA === undefined) return 1 * orderFactor;
      if (valB === null || valB === undefined) return -1 * orderFactor;

      if (typeof valA === 'number' && typeof valB === 'number') {
        return (valA - valB) * orderFactor;
      }
      return String(valA).localeCompare(String(valB)) * orderFactor;
    });
  }

  // Prepend Average row if enabled
  if (showAveragePnL.value && averageMetrics.value.sharpe !== 0) {
    const avgRow = {
      id: 'avg_row',
      code: 'Avg (Selected)',
      sharpe: averageMetrics.value.sharpe,
      returns: averageMetrics.value.returns,
      turnover: averageMetrics.value.turnover,
      margin: averageMetrics.value.margin,
      fitness: averageMetrics.value.fitness,
      drawdown: averageMetrics.value.drawdown,
      // 其他字段保持 undefined 以渲染为 "--"
    };
    return [avgRow, ...sortedData];
  }

  return sortedData;
});
const sortedAlphaList = computed(() => {
  const combinedMap = new Map();

  // Add search results first
  if (searchResults.value) {
    searchResults.value.forEach(a => combinedMap.set(a.id, a));
  }

  // Add all selected alphas (to ensure ones added via right-click show up)
  if (selectedAlphasMap.value) {
    selectedAlphasMap.value.forEach(a => combinedMap.set(a.id, a));
  }

  const combinedList = Array.from(combinedMap.values());

  return combinedList.sort((a, b) => {
    const aIsSelected = selectedAlphaIds.value.has(a.id);
    const bIsSelected = selectedAlphaIds.value.has(b.id);

    if (aIsSelected && !bIsSelected) {
      return -1; // a comes first
    }
    if (!aIsSelected && bIsSelected) {
      return 1; // b comes first
    }
    return 0; // maintain original order for items with the same selection status
  });
});
const visibleSelectedCount = computed(() => {
  return selectedAlphaIds.value.size;
});

const clusterAlphas = computed(() => {
  return searchResults.value.filter(alpha => 
    alpha.cluster_x != null && alpha.cluster_y != null && alpha.cluster_id != null
  );
});

const clusterLegendData = computed(() => {
  if (clusterAlphas.value.length === 0) return [];
  
  const clusterMap = new Map();
  clusterAlphas.value.forEach(alpha => {
    const clusterId = alpha.cluster_id;
    if (!clusterMap.has(clusterId)) {
      clusterMap.set(clusterId, { total: 0, selected: 0 });
    }
    const counts = clusterMap.get(clusterId);
    counts.total++;
    if (selectedAlphaIds.value.has(alpha.id)) {
      counts.selected++;
    }
  });
  
  const colors = [
    "#5470c6", "#91cc75", "#fac858", "#ee6666", "#73c0de",
    "#3ba272", "#fc8452", "#9a60b4", "#ea7ccc", "#5D9CEC",
    "#48CFAD", "#FFCE54", "#A0D468", "#4FC1E9", "#AC92EC"
  ];
  let colorIndex = 0;

  // Sort by clusterId if it's a number, otherwise sort by string
  const sortedClusterIds = Array.from(clusterMap.keys()).sort((a, b) => {
    if (typeof a === 'number' && typeof b === 'number') {
      return a - b;
    }
    return String(a).localeCompare(String(b));
  });

  return sortedClusterIds.map(id => {
    const seriesName = `Cluster ${id}`;
    const color = colors[colorIndex % colors.length];
    colorIndex++;
    const counts = clusterMap.get(id);
    return {
      id: id,
      count: counts.total,
      selectedCount: counts.selected,
      name: seriesName,
      color: color
    };
  });
});

const clusterChartOption = computed(() => {
  if (clusterAlphas.value.length === 0) return {};

  // Group data by cluster_id
  const clusters = new Map();
  clusterAlphas.value.forEach(p => {
    const clusterId = p.cluster_id;
    if (!clusters.has(clusterId)) {
      clusters.set(clusterId, []);
    }
    clusters.get(clusterId).push({
      name: p.id,
      value: [p.cluster_x, p.cluster_y, selectedAlphaIds.value.has(p.id) ? 1 : 0], // x, y, isSelected
    });
  });

  const series = clusterLegendData.value.map(legendItem => ({
    name: legendItem.name,
    type: 'scatter',
    data: clusters.get(legendItem.id) || [],
    itemStyle: {
      color: legendItem.color
    },
    emphasis: {
      focus: 'series',
      scale: true,
      label: {
        show: true,
        formatter: (param) => param.data.name,
        position: 'top'
      }
    }
  }));

  return {
    grid: { left: 10, right: 10, top: 10, bottom: 20 },
    legend: {
        show: false // Disable the built-in legend
    },
    tooltip: {
       formatter: (params) => {
          return `<div style="font-weight:bold">${params.data.name}</div>${params.seriesName}<br/>(${params.data.value[0].toFixed(2)}, ${params.data.value[1].toFixed(2)})`;
       }
    },
    xAxis: {
      scale: true,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { show: false },
      splitLine: { show: false }
    },
    yAxis: {
      scale: true,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { show: false },
      splitLine: { show: false }
    },
    visualMap: {
        type: 'piecewise',
        show: false,
        dimension: 2,
        pieces: [
            {value: 1, colorAlpha: 1}, 
            {value: 0, colorAlpha: 0.3},
        ],
    },
    dataZoom: [
      {
        id: 'dataZoomX',
        type: 'inside',
        xAxisIndex: 0,
      },
      {
        id: 'dataZoomY',
        type: 'inside',
        yAxisIndex: 0,
      },
    ],
    series
  };
});
// --- Computed ---
const alphasToDisplay = computed(() => {
  if (!searchResults.value) return [];

  if (showSelectedOnly.value) {
    return Array.from(selectedAlphasMap.value.values());
  } else {
    const selected = Array.from(selectedAlphasMap.value.values());
    const top30 = searchResults.value.slice(0, 30);
    
    const combined = new Map();
    top30.forEach(a => combined.set(a.id, a));
    selected.forEach(a => combined.set(a.id, a));
    
    return Array.from(combined.values())
        .filter(a => a && a.id)
        .sort((a, b) => {
            const idxA = searchResults.value.findIndex(x => x.id === a.id);
            const idxB = searchResults.value.findIndex(x => x.id === b.id);
            if (idxA !== -1 && idxB !== -1) return idxA - idxB;
            if (idxA !== -1) return -1;
            if (idxB !== -1) return 1;
            return a.id.localeCompare(b.id);
    });
  }
});

const chartOption = computed(() => {
  if (!alphasToDisplay.value || alphasToDisplay.value.length === 0 || !pnlDataMap.value) return {};

  const series = [];
  const allDates = new Set();

  // 收集所有日期并准备系列数据
  alphasToDisplay.value.forEach((alpha) => {
    if (!alpha || !alpha.id) return;
    const pnlData = pnlDataMap.value[alpha.id];
    if (pnlData && pnlData.length > 0) {
      pnlData.forEach((point) => {
        allDates.add(point.date);
      });
    }
  });

  const sortedDates = Array.from(allDates).sort();

  // 为每个Alpha创建系列
  alphasToDisplay.value.forEach((alpha) => {
    if (!alpha || !alpha.id) return;
    const pnlData = pnlDataMap.value[alpha.id];
    if (pnlData && pnlData.length > 0) {
      const alphaName = alpha.id;

      // 创建映射以便快速查找
      const pnlMap = new Map();
      pnlData.forEach((point) => {
        pnlMap.set(point.date, point.pnl);
      });

      // 创建与所有日期对应的值数组
      const values = sortedDates.map((date) => pnlMap.get(date) || null);

      const isSelected = selectedAlphaIds.value.has(alpha.id);
      
      series.push({
        name: alphaName,
        type: "line",
        data: values,
        color: getColorForAlpha(alpha.id, isSelected), // 设置系列颜色，确保tooltip指示器颜色一致
        showSymbol: true, // Show symbol but make it transparent
        symbol: "circle",
        symbolSize: 8, // Large enough to be easily hovered
        itemStyle: {
          opacity: 0 // Invisible normally
        },
        lineStyle: {
          width: isSelected ? 2.5 : 1, // 稍微加粗选中线
        },
        emphasis: {
          focus: "series",
          scale: true,
          itemStyle: {
            opacity: 1 // Visible on hover
          },
          lineStyle: {
            width: 4, // 悬停时明显加粗
            shadowBlur: 10,
            shadowColor: 'rgba(0,0,0,0.3)'
          }
        },
        triggerLineEvent: true, // 启用线条点击事件
      });
    }
  });

  // Calculate Average PnL if enabled and there are selected alphas
  if (showAveragePnL.value) {
    const selectedAlphas = alphasToDisplay.value.filter(alpha => selectedAlphaIds.value.has(alpha.id));
    if (selectedAlphas.length > 0) {
      // Pre-calculate maps for selected alphas to speed up lookup
      const alphaPnLMaps = selectedAlphas.map(alpha => {
        const pnlData = pnlDataMap.value[alpha.id];
        const m = new Map();
        if (pnlData) {
          pnlData.forEach(p => m.set(p.date, p.pnl));
        }
        return m;
      });

      const avgPnLValues = sortedDates.map(date => {
        let sum = 0;
        let count = 0;
        alphaPnLMaps.forEach(m => {
          const val = m.get(date);
          if (val != null) {
            sum += val;
            count++;
          }
        });
        return count > 0 ? sum / count : null;
      });

      series.push({
        name: "Average PnL",
        type: "line",
        data: avgPnLValues,
        color: "#000",
        z: 10, // Ensure it's on top
        lineStyle: {
          width: 3,
          type: 'dashed'
        },
        emphasis: {
          lineStyle: {
            width: 5
          }
        }
      });
    }
  }

  return {
    grid: { left: 50, right: 60, top: 40, bottom: 50, containLabel: true },
    tooltip: {
      show: true,
      trigger: 'item',
      confine: true,
      enterable: true,
      appendToBody: true, // Ensure tooltip is not clipped and has correct z-index
      extraCssText: 'z-index: 9999; box-shadow: 0 0 10px rgba(0,0,0,0.2);',
      axisPointer: {
        type: 'line',
        lineStyle: {
          color: '#aaa',
          type: 'dashed'
        }
      },
      formatter: (params) => {
        const p = Array.isArray(params) ? params[0] : params;
        if (!p || !p.seriesName) return '';

        const alphaId = p.seriesName;

        // Special handling for Average PnL
        if (alphaId === "Average PnL") {
          const formatPercent = (val) => val != null ? (val * 100).toFixed(2) + "%" : "--";
          const formatMargin = (val) => val != null ? (val * 10000).toFixed(2) + "‱" : "--";
          const formatNum = (val) => val != null ? Number(val).toFixed(3) : "--";

          return `
            <div style="padding: 4px; max-width: 400px;">
              <div style="font-weight: bold; margin-bottom: 6px; color: ${p.color}; border-bottom: 1px solid #eee; padding-bottom: 4px;">
                Average PnL
              </div>
              <div style="font-size: 12px; line-height: 1.6; color: #333;">
                <div style="display: flex; justify-content: space-between; gap: 15px;">
                  <span>Value:</span> 
                  <span style="font-weight: 500;">${p.value != null ? Number(p.value).toFixed(2) : '--'}</span>
                </div>
                <div style="display: flex; justify-content: space-between; gap: 15px; margin-top: 4px; padding-top: 4px; border-top: 1px dashed #eee;">
                  <span>Sharpe:</span> <span style="font-weight: 500;">${formatNum(averageMetrics.value.sharpe)}</span>
                </div>
                <div style="display: flex; justify-content: space-between; gap: 15px;">
                  <span>Fitness:</span> <span style="font-weight: 500;">${formatNum(averageMetrics.value.fitness)}</span>
                </div>
                <div style="display: flex; justify-content: space-between; gap: 15px;">
                  <span>Returns:</span> <span style="font-weight: 500;">${formatPercent(averageMetrics.value.returns)}</span>
                </div>
                <div style="display: flex; justify-content: space-between; gap: 15px;">
                  <span>Turnover:</span> <span style="font-weight: 500;">${formatPercent(averageMetrics.value.turnover)}</span>
                </div>
                <div style="display: flex; justify-content: space-between; gap: 15px;">
                  <span>Margin:</span> <span style="font-weight: 500;">${formatMargin(averageMetrics.value.margin)}</span>
                </div>
                <div style="display: flex; justify-content: space-between; gap: 15px;">
                  <span>Drawdown:</span> <span style="font-weight: 500;">${formatPercent(averageMetrics.value.drawdown)}</span>
                </div>
              </div>
            </div>
          `;
        }

        // Search in searchResults first, then in selectedAlphasMap
        let alpha = searchResults.value.find(a => a.id === alphaId);
        if (!alpha && selectedAlphasMap.value.has(alphaId)) {
            alpha = selectedAlphasMap.value.get(alphaId);
        }
        
        // Basic display if alpha details not found
        if (!alpha) {
          return `<div style="font-weight: bold; padding: 4px;">${alphaId}</div>`;
        }

        const formatPercent = (val) => val != null ? (val * 100).toFixed(2) + "%" : "--";
        const formatMargin = (val) => val != null ? (val * 10000).toFixed(2) + "‱" : "--";
        const formatNum = (val) => val != null ? Number(val).toFixed(3) : "--";
        const expression = alpha.code || "--";
        
        return `
          <div style="padding: 4px; max-width: 400px;">
            <div style="font-weight: bold; margin-bottom: 6px; color: ${p.color}; border-bottom: 1px solid #eee; padding-bottom: 4px;">
              ${alphaId}
            </div>
            <div style="font-size: 12px; line-height: 1.6; color: #333;">
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Region:</span> <span style="font-weight: 500;">${alpha.region || '--'}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Sharpe:</span> <span style="font-weight: 500;">${alpha.sharpe || '--'}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Fitness:</span> <span style="font-weight: 500;">${formatNum(alpha.fitness)}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Sub-U Sharpe:</span> <span style="font-weight: 500;">${formatNum(alpha.sub_universe_sharpe)}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Turnover:</span> <span style="font-weight: 500;">${formatPercent(alpha.turnover)}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Margin:</span> <span style="font-weight: 500;">${formatMargin(alpha.margin)}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Drawdown:</span> <span style="font-weight: 500;">${formatPercent(alpha.drawdown)}</span></div>
              
              <div style="margin-top: 8px; padding-top: 6px; border-top: 1px solid #eee;">
                <div style="margin-bottom: 2px; color: #666;">Expression:</div>
                <div style="font-family: monospace; white-space: pre-wrap; word-break: break-all; max-height: 100px; overflow-y: auto; background: #f5f5f5; padding: 4px; border-radius: 4px;">${expression}</div>
              </div>
            </div>
          </div>
        `;
      }
    },
    legend: {
      show: false, // 不显示图例，因为Alpha太多时会看不过来
    },
    dataZoom: [
      {
        type: "inside",
        xAxisIndex: 0,
        start: chartZoomState.value.xAxisStart,
        end: chartZoomState.value.xAxisEnd,
      },
      {
        type: "slider",
        xAxisIndex: 0,
        start: chartZoomState.value.xAxisStart,
        end: chartZoomState.value.xAxisEnd,
        height: 15,
        bottom: 35, // Adjusted to be closer to x-axis labels
        showDataShadow: false,
        showDetail: true, // Show detail
      },
      {
        type: "inside",
        yAxisIndex: 0,
        start: chartZoomState.value.yAxisStart,
        end: chartZoomState.value.yAxisEnd,
      },
      {
        type: "slider",
        yAxisIndex: 0,
        start: chartZoomState.value.yAxisStart,
        end: chartZoomState.value.yAxisEnd,
        width: 15,
        right: 40, // Adjusted to be closer to y-axis labels
        showDataShadow: false,
        showDetail: true, // Show detail
      },
    ],
    xAxis: {
      type: "category",
      data: sortedDates,
      boundaryGap: false,
      axisLine: { onZero: false, lineStyle: { color: "#ccc" } },
      axisLabel: {
        rotate: 45,
        fontSize: 10,
        formatter: (value) => value.slice(0, 7), // 只显示年月
      },
    },
    yAxis: {
      type: "value",
      min: "dataMin",
      max: "dataMax",
      axisLabel: {
        fontSize: 10,
        formatter: (value) => {
          const absVal = Math.abs(value);
          if (absVal >= 1e7) return (value / 1e6).toFixed(1) + "M";
          if (absVal >= 1e4) return (value / 1e3).toFixed(1) + "K";
          return value.toFixed(1);
        },
      },
      splitLine: { lineStyle: { color: "#eee" } },
    },
    series,
  };
});

// --- Methods ---
function handleSorterChange(sorter) {
  sortKey.value = sorter.columnKey;
  sortOrder.value = sorter.order;
}

function getColorForAlpha(alphaId, isSelected = true) {
  if (!alphaId) return "#999"; // Fallback color for invalid ID

  // 为每个Alpha分配一个固定的颜色
  const colors = [
    "#5470c6", "#91cc75", "#fac858", "#ee6666", "#73c0de",
    "#3ba272", "#fc8452", "#9a60b4", "#ea7ccc", "#5D9CEC",
    "#48CFAD", "#FFCE54", "#A0D468", "#4FC1E9", "#AC92EC"
  ];
  
  // 获取Alpha在搜索结果中的索引
  let index = searchResults.value.findIndex((a) => a.id === alphaId);
  
  // 如果不在搜索结果中（例如已选但未在当前页），使用ID的哈希值来分配颜色
  if (index === -1) {
    let hash = 0;
    for (let i = 0; i < alphaId.length; i++) {
      hash = alphaId.charCodeAt(i) + ((hash << 5) - hash);
    }
    index = Math.abs(hash);
  }
  
  const baseColor = colors[index % colors.length];
  
  // 如果未选中，则返回较淡的颜色
  if (!isSelected) {
    // 将颜色转换为RGBA并降低透明度
    const hex = baseColor.replace('#', '');
    const r = parseInt(hex.substring(0, 2), 16);
    const g = parseInt(hex.substring(2, 4), 16);
    const b = parseInt(hex.substring(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, 0.3)`;
  }
  
  return baseColor;
}

// 处理图表点击事件
function handleChartClick(params) {
  // 检查点击的是否是系列（线条）
  if (params.componentType === 'series') {
    const alphaId = params.seriesName;
    
    // 先获取当前缩放状态
    let currentZoomState = null;
    if (chartRef.value) {
      // vue-echarts的API可能不同，尝试不同的方法
      let chart = null;
      
      // 方法1: 直接使用chartRef.value作为echarts实例
      if (chartRef.value && typeof chartRef.value.dispatchAction === 'function') {
        chart = chartRef.value;
      }
      // 方法2: 尝试getEchartsInstance方法
      else if (chartRef.value && typeof chartRef.value.getEchartsInstance === 'function') {
        chart = chartRef.value.getEchartsInstance();
      }
      // 方法3: 尝试$el属性
      else if (chartRef.value && chartRef.value.$el && chartRef.value.$el.echarts) {
        chart = chartRef.value.$el.echarts;
      }
      
      if (chart) {
        try {
          const option = chart.getOption();
          if (option && option.dataZoom) {
            currentZoomState = {
              xAxisStart: option.dataZoom[0]?.start || 0,
              xAxisEnd: option.dataZoom[0]?.end || 100,
              yAxisStart: option.dataZoom[2]?.start || 0,
              yAxisEnd: option.dataZoom[2]?.end || 100
            };
          }
        } catch (e) {
          console.warn('获取图表选项失败:', e);
        }
      }
    }
    
    // 切换选中状态
    toggleAlphaSelection(alphaId);
    
    // 如果有保存的缩放状态，在下一个tick恢复它
    if (currentZoomState) {
      nextTick(() => {
        if (chartRef.value) {
          // 尝试获取echarts实例
          let chart = null;
          
          if (chartRef.value && typeof chartRef.value.dispatchAction === 'function') {
            chart = chartRef.value;
          } else if (chartRef.value && typeof chartRef.value.getEchartsInstance === 'function') {
            chart = chartRef.value.getEchartsInstance();
          } else if (chartRef.value && chartRef.value.$el && chartRef.value.$el.echarts) {
            chart = chartRef.value.$el.echarts;
          }
          
          if (chart) {
            try {
              chart.dispatchAction({
                type: 'dataZoom',
                start: currentZoomState.xAxisStart,
                end: currentZoomState.xAxisEnd,
                dataZoomIndex: 0
              });
              chart.dispatchAction({
                type: 'dataZoom',
                start: currentZoomState.yAxisStart,
                end: currentZoomState.yAxisEnd,
                dataZoomIndex: 2
              });
            } catch (e) {
              console.warn('恢复缩放状态失败:', e);
            }
          }
        }
      });
    }
  }
}

// 处理聚类图表点击事件
function handleClusterChartClick(params) {
  if (params.componentType === 'series' && params.seriesType === 'scatter') {
    const alphaId = params.data.name;
    if (alphaId) {
      toggleAlphaSelection(alphaId);
    }
  }
}

// 处理图表缩放事件
function handleDataZoom(params) {
  // params可能不是数组，需要检查类型
  if (!params) return;
  
  // 如果params是数组，遍历处理
  if (Array.isArray(params)) {
    params.forEach(param => {
      processZoomParam(param);
    });
  } else {
    // 如果params是单个对象，直接处理
    processZoomParam(params);
  }
}

// 处理单个缩放参数
function processZoomParam(param) {
  if (param.dataZoomIndex === 0 || param.dataZoomIndex === 1) {
    // X轴缩放
    chartZoomState.value.xAxisStart = param.start;
    chartZoomState.value.xAxisEnd = param.end;
  } else if (param.dataZoomIndex === 2 || param.dataZoomIndex === 3) {
    // Y轴缩放
    chartZoomState.value.yAxisStart = param.start;
    chartZoomState.value.yAxisEnd = param.end;
  }
}

async function searchAlphas(keepSelection = false) {
  if (!keepSelection) {
    // Clear previous selections for a new search
    selectedAlphaIds.value = new Set();
    selectedAlphasMap.value = new Map();
  }

  let queryToUse = searchQuery.value.trim();
  let collectionToUse = selectedCollection.value;
  let regionToUse = region.value.trim() || null;
  let delayToUse = delay.value.trim() ? parseInt(delay.value.trim()) : null;
  let pageSize = 1000;

  // 只保存搜索框内的字符串到历史记录
  if (!keepSelection && queryToUse && queryToUse !== "{}") {
    queryStore.addQuery('analysis', queryToUse);
  }

  // If refreshing, build query from existing results, ignoring UI filters
  if (keepSelection && searchResults.value.length > 0) {
    const currentAlphaIds = searchResults.value.map(alpha => alpha.id);
    const inQuery = { id: { $in: currentAlphaIds } };
    queryToUse = JSON.stringify(inQuery);
    regionToUse = null;
    delayToUse = null;
    pageSize = currentAlphaIds.length;
  }

  searchLoading.value = true;
  try {
    // 如果查询为空或者是{}，则查询所有数据
    const searchParam = (queryToUse === "" || queryToUse === "{}") ? null : queryToUse;
    
    const params = {
      collection: collectionToUse,
      embedding: embedding?.value || null,
      query: searchParam,
      region: regionToUse,
      delay: delayToUse,
      page: 1,
      page_size: pageSize,
    };

    console.log("搜索参数:", params);
    const result = await invoke("get_alpha_results", { params });
    console.log("搜索结果:", result);
    
    // Inject collection into each result item so loadPNL knows where to find it
    const resultsWithCollection = (result.data || []).map(item => ({
      ...item,
      collection: collectionToUse
    }));
    
    console.log("Full search results from backend:", resultsWithCollection);
    searchResults.value = resultsWithCollection;
  } catch (e) {
    console.error("Error searching alphas:", e);
    // On new search, clear results. On refresh, maybe keep old data?
    if (!keepSelection) {
      searchResults.value = [];
    }
  } finally {
    searchLoading.value = false;
  }
}

function toggleAlphaSelection(alphaId) {
  const newIdSet = new Set(selectedAlphaIds.value);
  const newMap = new Map(selectedAlphasMap.value);

  if (newIdSet.has(alphaId)) {
    newIdSet.delete(alphaId);
    newMap.delete(alphaId);
  } else {
    // Try to find alpha object in searchResults
    const alpha = searchResults.value.find(a => a.id === alphaId);
    if (alpha) {
      newIdSet.add(alphaId);
      newMap.set(alphaId, alpha);
    } else {
        // If not in search results, check if it's already in the map (shouldn't happen if we are toggling, but good for safety)
        // Or if triggered from chart for an item not in search results but in map?
        if (newMap.has(alphaId)) {
             // It's in the map but not in search results? That means it was already selected. 
             // Logic above says if it has alphaId, delete it. So we are in the else branch (add).
             // If we are adding and it's not in search results, we can't add it unless we fetch it.
             // But usually we toggle visible things.
             console.warn(`Attempted to select Alpha ${alphaId} which is not in search results.`);
        }
    }
  }
  selectedAlphaIds.value = newIdSet;
  selectedAlphasMap.value = newMap;
}

function removeAlpha(alphaId) {
  // 从搜索结果中移除
  searchResults.value = searchResults.value.filter(alpha => alpha.id !== alphaId);

  // 从选择中移除（如果存在）
  if (selectedAlphaIds.value.has(alphaId)) {
    const newIdSet = new Set(selectedAlphaIds.value);
    const newMap = new Map(selectedAlphasMap.value);
    newIdSet.delete(alphaId);
    newMap.delete(alphaId);
    selectedAlphaIds.value = newIdSet;
    selectedAlphasMap.value = newMap;
  }
}

function clearSelections() {
  selectedAlphaIds.value = new Set();
  selectedAlphasMap.value = new Map();
  // 不重置缩放状态，保持用户当前的缩放位置
}

function clearAlphaList() {
  searchResults.value = [];
  selectedAlphaIds.value = new Set(); // Also clear selections when clearing the list
  selectedAlphasMap.value = new Map(); // Also clear selections when clearing the list
  message.success('已清空 Alpha 列表');
}

function selectAll() {
  const newIdSet = new Set(selectedAlphaIds.value);
  const newMap = new Map(selectedAlphasMap.value);
  searchResults.value.forEach(alpha => {
    if (!newIdSet.has(alpha.id)) {
      newIdSet.add(alpha.id);
      newMap.set(alpha.id, alpha);
    }
  });
  selectedAlphaIds.value = newIdSet;
  selectedAlphasMap.value = newMap;
  message.success(`已选择全部 ${searchResults.value.length} 个Alpha`);
  // 不重置缩放状态，保持用户当前的缩放位置
}

function deselectAll() {
  selectedAlphaIds.value = new Set();
  selectedAlphasMap.value = new Map();
  message.success('已清除所有选择');
  // 不重置缩放状态，保持用户当前的缩放位置
}

async function addAlpha() {
  const alphaId = newAlphaId.value.trim();
  if (!alphaId) return;
  
  // 检查是否已经在选择中
  if (selectedAlphaIds.value.has(alphaId)) {
    message.info(`Alpha ID "${alphaId}" 已在选择中`);
    newAlphaId.value = ""; // 清空输入框
    return;
  }
  
  // 检查是否在搜索结果中存在
  const exists = searchResults.value.some(alpha => alpha.id === alphaId);
  
  if (!exists) {
    // 如果不在搜索结果中，尝试从alpha_db的所有collection中搜索
    try {
      const result = await invoke("search_alpha_in_all_collections", { query: { id: alphaId } });
      if (result) {
        // 找到了alpha，添加到搜索结果中，并存储collection信息
        searchResults.value.push({
          id: alphaId,
          region: result.region || "Unknown",
          sharpe: result.sharpe,
          fitness: result.fitness,
          sub_universe_sharpe: result.sub_universe_sharpe,
          returns: result.returns,
          turnover: result.turnover,
          margin: result.margin,
          code: result.code,
          message: result.message,
          date_created: result.date_created,
          collection: result.collection // 存储collection信息
        });
        
        // 加载这个alpha的PNL数据，传入正确的collection
        loadPNL(alphaId, result.collection);
      } else {
        message.warning(`Alpha ID "${alphaId}" 未在alpha_db中找到`);
        return;
      }
    } catch (error) {
      console.error(`Error searching for alpha ${alphaId}:`, error);
      message.error(`搜索Alpha ID "${alphaId}" 时出错`);
      return;
    }
  }
  
  // Ensure the alpha object is available (it was just added to searchResults if it wasn't there)
  const alpha = searchResults.value.find(a => a.id === alphaId);
  if (alpha) {
    const newIdSet = new Set(selectedAlphaIds.value);
    const newMap = new Map(selectedAlphasMap.value);
    newIdSet.add(alphaId);
    newMap.set(alphaId, alpha);
    selectedAlphaIds.value = newIdSet;
    selectedAlphasMap.value = newMap;
  }

  newAlphaId.value = ""; // 清空输入框
  message.success(`已添加 Alpha ID "${alphaId}"`);
}

function removeSelectedAlpha() {
  if (selectedAlphaIds.value.size === 0) {
    message.warning('请先选择要删除的Alpha');
    return;
  }
  
  const selectedArray = Array.from(selectedAlphaIds.value);
  const originalLength = searchResults.value.length;
  
  // 从搜索结果中移除选中的Alpha (Visual removal from list if present)
  searchResults.value = searchResults.value.filter(alpha => !selectedAlphaIds.value.has(alpha.id));
  
  // 清除选择
  selectedAlphaIds.value = new Set();
  selectedAlphasMap.value = new Map();
  
  const removedCount = originalLength - searchResults.value.length;
  message.success(`已从列表中删除 ${removedCount} 个Alpha`);
}

function clearFilters() {
  analysisStore.reset();
}

async function exportSelection() {
  if (selectedAlphaIds.value.size === 0) {
    message.warning('请先选择Alpha');
    return;
  }
  
  try {
    const selectedIds = Array.from(selectedAlphaIds.value);
    const dataStr = JSON.stringify(selectedIds, null, 2);
    const fileName = `alpha_selection_${new Date().toISOString().slice(0, 10)}.json`;
    
    // Use Tauri's save_dialog to let user pick location and name
    const filePath = await invoke('save_dialog', {
      defaultPath: fileName,
      filters: [
        {
          name: 'JSON',
          extensions: ['json']
        }
      ]
    });
    
    if (filePath) {
      await invoke('write_file', {
        path: filePath,
        contents: dataStr
      });
      message.success(`已导出 ${selectedIds.length} 个Alpha ID`);
    }
  } catch (error) {
    console.error('导出失败:', error);
    message.error('导出失败，请重试');
  }
}

async function importSelection() {
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.json';
  input.onchange = async (event) => {
    const file = event.target.files[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = async (e) => {
      try {
        const ids = JSON.parse(e.target.result);
        if (!Array.isArray(ids)) {
          message.error('导入文件格式错误');
          return;
        }

        searchLoading.value = true;
        
        // Step 1: Validate IDs and get their collections
        const initialSearchPromises = ids.map(id =>
          invoke("search_alpha_in_all_collections", { query: { id } })
        );
        const initialResults = await Promise.all(initialSearchPromises);

        const foundAlphas = initialResults.filter(r => r != null);
        const notFoundIds = ids.filter(id => !foundAlphas.some(a => a.id === id));

        // Step 2: Group found alphas by collection
        const alphasByCollection = new Map();
        foundAlphas.forEach(alpha => {
          if (!alphasByCollection.has(alpha.collection)) {
            alphasByCollection.set(alpha.collection, []);
          }
          alphasByCollection.get(alpha.collection).push(alpha.id);
        });

        // Step 3: Fetch full alpha data with embedding, in parallel for each collection
        const fetchPromises = [];
        for (const [collection, alphaIds] of alphasByCollection.entries()) {
          const params = {
            collection: collection,
            embedding: embedding?.value || null,
            query: JSON.stringify({ id: { $in: alphaIds } }),
            region: null,
            delay: null,
            page: 1,
            page_size: alphaIds.length,
          };
          fetchPromises.push(invoke("get_alpha_results", { params }));
        }

        const pagedResults = await Promise.all(fetchPromises);
        
        // Step 4: Combine results and update UI
        const finalResults = pagedResults.flatMap(p => p.data || []);
        
        const resultsWithCollection = finalResults.map((item, index) => {
            // Find the original collection since get_alpha_results does not return it.
            const originalAlpha = foundAlphas.find(a => a.id === item.id);
            return {
                ...item,
                collection: originalAlpha ? originalAlpha.collection : 'unknown'
            };
        });

        searchResults.value = resultsWithCollection;
        selectedAlphaIds.value = new Set();
        selectedAlphasMap.value = new Map();

        // Step 5: Show result message
        if (notFoundIds.length > 0) {
          message.warning(`成功导入 ${foundAlphas.length} 个Alpha，${notFoundIds.length} 个ID未在alpha_db中找到: ${notFoundIds.join(', ')}`);
        } else {
          message.success(`成功导入 ${foundAlphas.length} 个Alpha`);
        }

      } catch (error) {
        console.error("导入失败:", error);
        message.error('导入文件解析失败或过程中出错');
      } finally {
        searchLoading.value = false;
      }
    };
    reader.readAsText(file);
  };
  input.click();
}
async function loadPNL(id, collection = null) {
  console.trace(`loadPNL called for ID: ${id}`);
  if (loadingSet.value.has(id) || pnlDataMap.value[id] !== undefined) return;
  loadingSet.value.add(id);
  try {
    // 如果没有指定collection，尝试从搜索结果中获取存储的collection信息
    let queryCollection = collection;
    
    if (!queryCollection) {
      // 首先尝试从搜索结果中获取collection信息
      const alphaInSearchResults = searchResults.value.find(alpha => alpha.id === id);
      if (alphaInSearchResults && alphaInSearchResults.collection) {
        queryCollection = alphaInSearchResults.collection;
        console.log(`从搜索结果中找到Alpha ${id} 在collection: ${queryCollection}`);
      } else {
        // 如果搜索结果中没有，则从alpha_db的所有collection中搜索
        try {
          const result = await invoke("search_alpha_in_all_collections", { query: { id } });
          if (result && result.collection) {
            queryCollection = result.collection;
            console.log(`从数据库中找到Alpha ${id} 在collection: ${queryCollection}`);
          } else {
            console.error(`Alpha ${id} 未在任何collection中找到`);
            pnlDataMap.value[id] = null;
            loadingSet.value.delete(id);
            return;
          }
        } catch (error) {
          console.error(`Error finding collection for alpha ${id}:`, error);
          pnlDataMap.value[id] = null;
          loadingSet.value.delete(id);
          return;
        }
      }
    }
    
    const query = { id, collection: queryCollection };
    console.log("加载PNL数据，查询参数:", query);
    const result = await invoke("get_pnl_by_id", { query });
    console.log(`加载PNL数据成功，Alpha ID: ${id}, 数据点数: ${result.pnl_series?.length || 0}`);
    pnlDataMap.value[id] = markRaw(result.pnl_series); // Use markRaw to prevent deep reactivity
  } catch (e) {
    console.error("Error loading PnL:", e);
    pnlDataMap.value[id] = null;
  } finally {
    loadingSet.value.delete(id);
  }
}

// --- Watchers ---
watch(embedding, (newValue, oldValue) => {
  if (newValue !== oldValue && searchResults.value.length > 0) {
    searchAlphas(true); // Keep selection when changing embedding
  }
});

watch([showAveragePnL, selectedAlphaIds, pnlDataMap], async () => {
  if (!showAveragePnL.value || selectedAlphaIds.value.size === 0) {
    averageMetrics.value = { sharpe: 0, returns: 0, turnover: 0, margin: 0, fitness: 0, drawdown: 0 };
    return;
  }

  const selectedAlphas = alphasToDisplay.value.filter(alpha => selectedAlphaIds.value.has(alpha.id));
  if (selectedAlphas.length === 0) {
    averageMetrics.value = { sharpe: 0, returns: 0, turnover: 0, margin: 0, fitness: 0, drawdown: 0 };
    return;
  }

  // Find all dates
  const allDates = new Set();
  selectedAlphas.forEach((alpha) => {
    const pnlData = pnlDataMap.value[alpha.id];
    if (pnlData && pnlData.length > 0) {
      pnlData.forEach((point) => {
        allDates.add(point.date);
      });
    }
  });

  if (allDates.size === 0) {
    averageMetrics.value = { sharpe: 0, returns: 0, turnover: 0, margin: 0, fitness: 0, drawdown: 0 };
    return;
  }

  const sortedDates = Array.from(allDates).sort();

  // Pre-calculate maps for selected alphas to speed up lookup
  const alphaPnLMaps = selectedAlphas.map(alpha => {
    const pnlData = pnlDataMap.value[alpha.id];
    const m = new Map();
    if (pnlData) {
      pnlData.forEach(p => m.set(p.date, p.pnl));
    }
    return m;
  });

  const avgPnLSeries = sortedDates.map(date => {
    let sum = 0;
    let count = 0;
    alphaPnLMaps.forEach(m => {
      const val = m.get(date);
      if (val != null) {
        sum += val;
        count++;
      }
    });
    return {
      date,
      pnl: count > 0 ? sum / count : 0 // Fallback to 0 if no data
    };
  });

  if (avgPnLSeries.length < 2) {
    averageMetrics.value = { sharpe: 0, returns: 0, turnover: 0, margin: 0, fitness: 0, drawdown: 0 };
    return;
  }

  try {
    const result = await invoke("calculate_pnl_metrics", { pnlSeries: avgPnLSeries });
    
    // 计算选中项的 returns 平均值，以确保单位和格式与 metadata 一致
    const validReturns = selectedAlphas
      .map(a => a.returns)
      .filter(r => r != null && typeof r === 'number');
    
    const avgReturn = validReturns.length > 0 
      ? validReturns.reduce((sum, r) => sum + r, 0) / validReturns.length 
      : 0;

    const validTurnovers = selectedAlphas
      .map(a => a.turnover)
      .filter(t => t != null && typeof t === 'number');
    
    const avgTurnover = validTurnovers.length > 0
      ? validTurnovers.reduce((sum, t) => sum + t, 0) / validTurnovers.length
      : 0;

    // Margin = (Return * 2) / Turnover / 1000
    const avgMargin = avgTurnover > 0 ? ((avgReturn * 2) / avgTurnover) / 1000 : 0;

    // Fitness = Sharpe * sqrt(abs(Returns) / Max(Turnover, 0.125))
    const avgFitness = result.sharpe * Math.sqrt(Math.abs(avgReturn) / Math.max(avgTurnover, 0.125));

    averageMetrics.value = {
      sharpe: result.sharpe,
      returns: avgReturn,
      turnover: avgTurnover,
      margin: avgMargin,
      fitness: avgFitness,
      drawdown: result.drawdown
    };
  } catch (e) {
    console.error("Error calculating average metrics:", e);
    averageMetrics.value = { sharpe: 0, returns: 0, turnover: 0, margin: 0, fitness: 0, drawdown: 0 };
  }
}, { deep: true, immediate: true });

watch(alphasToDisplay, (alphas) => {
  if (alphas && alphas.length > 0) {
    alphas.forEach(alpha => {
      if (alpha && alpha.id && pnlDataMap.value[alpha.id] === undefined) {
        loadPNL(alpha.id);
      }
    });
  }
}, { deep: true, immediate: true });

// --- Lifecycle ---
onMounted(() => {
  // Set default embedding if not set
  if (!embedding.value && configStore.embeddingOptions.length > 0) {
    embedding.value = configStore.embeddingOptions[0].value;
  }
});

// Watch for embedding options loading to set default if still empty
watch(() => configStore.embeddingOptions, (options) => {
  if (!embedding.value && options && options.length > 0) {
    embedding.value = options[0].value;
  }
}, { immediate: true });
</script>

<style scoped>
.n-tag {
  margin: 4px;
}
.legend-item:hover {
  background-color: #f0f0f0 !important;
}
</style>