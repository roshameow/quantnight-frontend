<template>
  <div>
    <!-- 数据集和查询选择区域 -->
    <div style="background: #f8f8f8; padding: 16px; border-radius: 8px; margin-bottom: 16px">
      <n-grid :cols="24" :x-gap="12" :y-gap="12">
        <!-- 数据集选择 -->
        <n-grid-item span="6">
          <n-select
            v-model:value="selectedCollection"
            :options="configStore.dataFilterOptions"
            placeholder="选择数据集"
            style="width: 100%"
          />
        </n-grid-item>

        <!-- 查询输入 -->
        <n-grid-item span="6">
          <n-input
            v-model:value="searchQuery"
            placeholder="输入查询条件（留空或输入{}查询全部）"
            clearable
            style="width: 100%"
            @keyup.enter="searchAlphas"
          />
        </n-grid-item>

        <!-- Region输入 -->
        <n-grid-item span="4">
          <n-input
            v-model:value="region"
            placeholder="Region"
            clearable
            style="width: 100%"
            @keyup.enter="searchAlphas"
          />
        </n-grid-item>

        <!-- Delay输入 -->
        <n-grid-item span="4">
          <n-input
            v-model:value="delay"
            placeholder="Delay"
            clearable
            style="width: 100%"
            @keyup.enter="searchAlphas"
          />
        </n-grid-item>

        <!-- 搜索按钮 -->
        <n-grid-item span="2">
          <n-button type="primary" @click="searchAlphas" :loading="searchLoading" style="width: 100%">
            搜索
          </n-button>
        </n-grid-item>
      </n-grid>
      
      <!-- 第二行：清除选择按钮 -->
      <n-grid :cols="24" :x-gap="12" :y-gap="12" style="margin-top: 8px">
        <n-grid-item span="24" style="text-align: right">
          <n-button @click="clearSelections" :disabled="selectedAlphaIds.size === 0">
            清除选择
          </n-button>
        </n-grid-item>
      </n-grid>
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
              导入选择
            </n-button>
          </n-space>
        </div>
        
        <div v-if="searchResults.length === 0 && !searchLoading" style="color: #999; text-align: center; padding: 10px; font-size: 12px">
          暂无搜索结果
        </div>
        
        <div v-else>
          <!-- 列表框容器 -->
          <div style="position: relative; border: 1px solid #e0e0e0; border-radius: 4px">
            <!-- 滚动区域 -->
            <div style="height: 400px; overflow-y: auto">
              <div
                v-for="alpha in searchResults"
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
                  <span>{{ alpha.id }}</span>
                  <n-checkbox
                    :checked="selectedAlphaIds.has(alpha.id)"
                    @update:checked="toggleAlphaSelection(alpha.id)"
                    @click.stop
                    size="small"
                  />
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
            <div v-if="searchResults.length > 0" style="position: absolute; bottom: 0; right: 0; display: flex; gap: 0px">
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
        <div v-if="searchResults.length > 0" style="margin-top: 8px">
          <div style="display: flex; gap: 8px">
            <n-button size="tiny" @click="selectAll" :disabled="searchResults.length === 0" style="flex: 1">
              全选
            </n-button>
            <n-button size="tiny" @click="deselectAll" :disabled="selectedAlphaIds.size === 0" style="flex: 1">
              全不选
            </n-button>
          </div>
        </div>
      </div>

      <!-- PNL图表区域 -->
      <div style="flex: 1; min-width: 0">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px">
          <div style="display: flex; align-items: center; gap: 8px">
            <h3 style="margin: 0">PNL对比图</h3>
            <n-text v-if="!showSelectedOnly && searchResults.length > 30" type="warning" style="font-size: 12px">
              (未选中仅显示前30)
            </n-text>
          </div>
          <n-switch v-model:value="showSelectedOnly">
            <template #checked>
              仅显示选中
            </template>
            <template #unchecked>
              显示全部
            </template>
          </n-switch>
        </div>
        <div v-if="searchResults.length === 0" style="color: #999; text-align: center; padding: 40px">
          请先搜索Alpha以显示PNL图表
        </div>
        <div v-else style="width: 100%; height: 500px">
          <v-chart
            ref="chartRef"
            :option="chartOption"
            style="width: 100%; height: 100%"
            @click="handleChartClick"
            @datazoom="handleDataZoom"
            :autoresize="true"
          />
        </div>
      </div>
    </div>

    <!-- Selected Alphas Table -->
    <div v-if="selectedAlphaIds.size > 0" style="margin-top: 16px; background: #fff; padding: 16px; border-radius: 8px; border: 1px solid #eee;">
      <h3 style="margin-top: 0; margin-bottom: 12px">选中的 Alpha 详情</h3>
      <n-data-table
        :columns="filteredColumns"
        :data="selectedAlphaDetails"
        :bordered="false"
        :scroll-x="1200"
        class="custom-table"
        :row-key="(row) => row.id"
      />
    </div>
  </div>
</template>

<script setup>
import { ref, computed, watch, onMounted, nextTick, markRaw } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useConfigStore } from "../stores/configStore";
import { useAnalysisStore } from "../stores/analysisStore";
import { storeToRefs } from "pinia";
import VChart from "vue-echarts";
import { use } from "echarts/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent, LegendComponent, DataZoomComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import { useMessage, useDialog } from 'naive-ui';
import { useAlphaTableColumns } from "../composables/useAlphaTableColumns.js";

// Register ECharts components
use([LineChart, GridComponent, TooltipComponent, LegendComponent, DataZoomComponent, CanvasRenderer]);

// --- State ---
const configStore = useConfigStore();
const analysisStore = useAnalysisStore();
const {
  selectedCollection,
  searchQuery,
  region,
  delay,
  searchResults,
  selectedAlphaIds,
  selectedAlphasMap,
  pnlDataMap,
  showSelectedOnly,
  chartZoomState
} = storeToRefs(analysisStore);

const message = useMessage();
const dialog = useDialog();
const searchLoading = ref(false);
const newAlphaId = ref("");
const loadingSet = ref(new Set());
const chartRef = ref(null);

// --- Table Setup ---
const expandedRowIds = ref(new Set());
const { columns } = useAlphaTableColumns({
  expandedRowIds,
  pnlDataMap,
  loadingSet,
  loadPNL,
});

const filteredColumns = computed(() => {
  const excludedKeys = ['message', 'pnl', 'corr_ppac', 'corr_os'];
  return columns.filter(col => !excludedKeys.includes(col.key));
});

const selectedAlphaDetails = computed(() => {
  return Array.from(selectedAlphasMap.value.values());
});

const visibleSelectedCount = computed(() => {
  if (!searchResults.value) return 0;
  return searchResults.value.filter(alpha => selectedAlphaIds.value.has(alpha.id)).length;
});

// --- Computed ---
const chartOption = computed(() => {
  if (!searchResults.value || !pnlDataMap.value) return {};
  if (searchResults.value.length === 0 && selectedAlphaIds.value.size === 0) return {};

  const series = [];
  const allDates = new Set();

  // Filter alphas to display
  let alphasToDisplay = [];
  
  if (showSelectedOnly.value) {
    // If showing only selected, show all of them (unlimited)
    alphasToDisplay = Array.from(selectedAlphasMap.value.values());
  } else {
    // If showing all:
    // 1. Identify selected alphas
    const selected = Array.from(selectedAlphasMap.value.values());
    // 2. Identify top 30 from search results (default context)
    const top30 = searchResults.value.slice(0, 30);
    
    // 3. Union them
    const combined = new Map();
    // Add top 30 first
    top30.forEach(a => combined.set(a.id, a));
    // Add selected (overwriting or adding)
    selected.forEach(a => combined.set(a.id, a));
    
    // 4. Convert back to array and sort by original index to maintain list order consistency
    alphasToDisplay = Array.from(combined.values())
        .filter(a => a && a.id) // Filter out any invalid objects
        .sort((a, b) => {
            const idxA = searchResults.value.findIndex(x => x.id === a.id);
            const idxB = searchResults.value.findIndex(x => x.id === b.id);
            if (idxA !== -1 && idxB !== -1) return idxA - idxB;
            if (idxA !== -1) return -1; // A is in list, B is not -> A comes first
            if (idxB !== -1) return 1;  // B is in list, A is not -> B comes first
            return a.id.localeCompare(b.id); // Both not in list -> sort by ID
    });
  }

  // 收集所有日期并准备系列数据
  alphasToDisplay.forEach((alpha) => {
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
  alphasToDisplay.forEach((alpha) => {
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

  return {
    grid: { left: 50, right: 40, top: 40, bottom: 80, containLabel: true },
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
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Sub-U Sharpe:</span> <span style="font-weight: 500;">${formatNum(alpha.sub_universe_sharpe)}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Turnover:</span> <span style="font-weight: 500;">${formatPercent(alpha.turnover)}</span></div>
              <div style="display: flex; justify-content: space-between; gap: 15px;"><span>Margin:</span> <span style="font-weight: 500;">${formatMargin(alpha.margin)}</span></div>
              
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
        start: chartZoomState.value.xAxisStart,
        end: chartZoomState.value.xAxisEnd,
      },
      {
        start: chartZoomState.value.xAxisStart,
        end: chartZoomState.value.xAxisEnd,
      },
      {
        type: "inside", // 添加y轴内部缩放
        yAxisIndex: 0,
        start: chartZoomState.value.yAxisStart,
        end: chartZoomState.value.yAxisEnd,
      },
      {
        type: "slider", // 添加y轴滑动条缩放
        yAxisIndex: 0,
        start: chartZoomState.value.yAxisStart,
        end: chartZoomState.value.yAxisEnd,
        width: 20, // 滑动条宽度
        right: 10, // 距离右侧的距离
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

async function searchAlphas() {
  searchLoading.value = true;
  try {
    // 如果查询为空或者是{}，则查询所有数据
    const query = searchQuery.value.trim();
    const searchParam = (query === "" || query === "{}") ? null : query;
    const regionParam = region.value.trim() || null;
    const delayParam = delay.value.trim() ? parseInt(delay.value.trim()) : null;
    
    const params = {
      collection: selectedCollection.value,
      query: searchParam,
      region: regionParam,
      delay: delayParam,
      page: 1,
      page_size: 1000, // 获取更多结果以便选择
    };

    console.log("搜索参数:", params);
    const result = await invoke("get_alpha_results", { params });
    console.log("搜索结果:", result);
    
    // Inject collection into each result item so loadPNL knows where to find it
    const resultsWithCollection = (result.data || []).map(item => ({
      ...item,
      collection: selectedCollection.value
    }));
    
    searchResults.value = resultsWithCollection;
    console.log("处理后的搜索结果:", searchResults.value);
  } catch (e) {
    console.error("Error searching alphas:", e);
    searchResults.value = [];
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
  // 从选择中移除
  selectedAlphaIds.value.delete(alphaId);
  // 从搜索结果中移除
  searchResults.value = searchResults.value.filter(alpha => alpha.id !== alphaId);
}

function clearSelections() {
  selectedAlphaIds.value = new Set();
  // 不重置缩放状态，保持用户当前的缩放位置
}

function selectAll() {
  const newSet = new Set(selectedAlphaIds.value);
  searchResults.value.forEach(alpha => {
    newSet.add(alpha.id);
  });
  selectedAlphaIds.value = newSet;
  message.success(`已选择全部 ${searchResults.value.length} 个Alpha`);
  // 不重置缩放状态，保持用户当前的缩放位置
}

function deselectAll() {
  selectedAlphaIds.value = new Set();
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
    
    // 在Tauri应用中，使用Tauri的API保存文件
    try {
      // 使用简化的save_dialog API获取默认路径
      const defaultPath = await invoke('save_dialog', {
        defaultPath: fileName,
        filters: [
          {
            name: 'JSON',
            extensions: ['json']
          }
        ]
      });
      
      if (defaultPath) {
        // 使用Tauri的write_file API写入文件
        await invoke('write_file', {
          path: defaultPath,
          contents: dataStr
        });
        message.success(`已导出 ${selectedIds.length} 个Alpha ID`);
        return;
      }
    } catch (tauriError) {
      console.warn('Tauri API不可用，尝试其他方法:', tauriError);
    }
    
    // 回退到浏览器下载
    const dataBlob = new Blob([dataStr], { type: 'application/json' });
    const url = URL.createObjectURL(dataBlob);
    const link = document.createElement('a');
    link.href = url;
    link.download = fileName;
    link.style.display = 'none';
    document.body.appendChild(link);
    link.click();
    
    // 延迟清理，确保下载开始
    setTimeout(() => {
      document.body.removeChild(link);
      URL.revokeObjectURL(url);
    }, 100);
    
    message.success(`已导出 ${selectedIds.length} 个Alpha ID`);
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
        
        // 清除当前选择
        selectedAlphaIds.value.clear();
        
        // 验证每个ID是否在alpha_db的任何collection中存在
        const validIds = [];
        const notFoundIds = [];
        
        for (const id of ids) {
          try {
            const result = await invoke("search_alpha_in_all_collections", { query: { id } });
            if (result) {
              validIds.push(id);
              // 如果这个alpha不在当前搜索结果中，添加到搜索结果
              if (!searchResults.value.some(alpha => alpha.id === id)) {
                // 创建一个简化的alpha对象，只包含基本信息，并存储collection信息
                searchResults.value.push({
                  id: id,
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
              }
            } else {
              notFoundIds.push(id);
            }
          } catch (error) {
            console.error(`Error searching for alpha ${id}:`, error);
            notFoundIds.push(id);
          }
        }
        
        // 添加所有有效的ID到选择中
        const newIdSet = new Set(selectedAlphaIds.value);
        const newMap = new Map(selectedAlphasMap.value);
        
        validIds.forEach(id => {
            newIdSet.add(id);
            // Find the alpha object we just ensured exists in searchResults
            const alpha = searchResults.value.find(a => a.id === id);
            if (alpha) {
                newMap.set(id, alpha);
            }
        });
        
        selectedAlphaIds.value = newIdSet;
        selectedAlphasMap.value = newMap;
        
        // 显示结果
        if (notFoundIds.length > 0) {
          message.warning(`成功导入 ${validIds.length} 个Alpha ID，${notFoundIds.length} 个ID未在alpha_db中找到: ${notFoundIds.join(', ')}`);
        } else {
          message.success(`成功导入 ${validIds.length} 个Alpha ID`);
        }
      } catch (error) {
        message.error('导入文件解析失败');
      }
    };
    reader.readAsText(file);
  };
  input.click();
}

async function loadPNL(id, collection = null) {
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
watch(searchResults, (newResults) => {
  // 为所有搜索结果的Alpha加载PNL数据
  newResults.forEach((alpha) => {
    if (!pnlDataMap.value[alpha.id]) {
      loadPNL(alpha.id);
    }
  });
}, { deep: true });

// --- Lifecycle ---
onMounted(() => {
  // 初始化时可以执行一些操作
});

// 确保在搜索结果更新后加载PNL数据
watch([searchResults, selectedCollection], () => {
  if (searchResults.value.length > 0) {
    searchResults.value.forEach((alpha) => {
      if (!pnlDataMap.value[alpha.id]) {
        loadPNL(alpha.id);
      }
    });
  }
}, { deep: true });
</script>

<style scoped>
.n-tag {
  margin: 4px;
}
</style>