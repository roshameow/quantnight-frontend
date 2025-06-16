<template>
  <div
    style="background: #f8f8f8; padding: 3px 16px; border-radius: 8px; margin-bottom: 6px"
  >
    <n-grid :cols="24" :x-gap="12" :y-gap="8" item-responsive responsive="screen">
      <!-- 左侧：输入项 -->
      <n-grid-item span="22 m:20 l:21">
        <n-space align="center" wrap :size="[12, 12]">
          <n-input
            v-model:value="searchQuery"
            placeholder="🔍 Search Regular"
            clearable
            @update:value="fetchData"
            style="width: 280px"
            spellcheck="false"
          />
          <n-select
            v-model:value="statusFilter"
            :options="[
              { label: 'All Status', value: '' },
              { label: 'UNKNOWN', value: 'UNKNOWN' },
              { label: 'FAIL', value: 'FAIL' },
            ]"
            placeholder="Status"
            clearable
            style="width: 130px"
            @update:value="fetchData"
          />
          <n-input
            v-model:value="regionFilter"
            placeholder="Region"
            clearable
            @update:value="fetchData"
            style="width: 100px"
            spellcheck="false"
          />
          <n-input
            v-model:value="daysFilter"
            placeholder="Days Before"
            type="number"
            clearable
            @update:value="fetchData"
            style="width: 130px"
            spellcheck="false"
          />
          <n-input
            v-model:value="minTurnoverFilter"
            placeholder="Min Turnover (%)"
            type="number"
            clearable
            @update:value="fetchData"
            style="width: 130px"
            spellcheck="false"
          />
          <n-input
            v-model:value="maxTurnoverFilter"
            placeholder="Max Turnover (%)"
            type="number"
            clearable
            @update:value="fetchData"
            style="width: 130px"
            spellcheck="false"
          />
          <n-input
            v-model:value="minMarginFilter"
            placeholder="Min Margin (‱)"
            type="number"
            clearable
            @update:value="fetchData"
            style="width: 130px"
            spellcheck="false"
          />
        </n-space>
      </n-grid-item>

      <!-- 右侧：按钮 -->
      <n-grid-item span="2 m:4 l:3" style="text-align: right">
        <n-button type="default" @click="calculateCorr" :loading="corrLoading">
          计算 Correlation
        </n-button>
      </n-grid-item>
    </n-grid>
  </div>

  <n-data-table
    :columns="columns"
    :data="data"
    :pagination="{
      pageSize: 20,
      showQuickJumper: true,
    }"
    :bordered="false"
    :scroll-x="1200"
    class="custom-table"
    :row-key="(row) => row.id"
  />
</template>

<script setup>
import { ref, watchEffect, onMounted, h, nextTick } from "vue";
import { NPopover } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import VChart from "vue-echarts";
import { use } from "echarts/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent, LegendComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";

use([
  LineChart, // ✅ 线图必须注册
  GridComponent,
  TooltipComponent,
  CanvasRenderer,
  LegendComponent,
]);

const data = ref([]);
const searchQuery = ref("");
const statusFilter = ref("");
const regionFilter = ref("");
const daysFilter = ref(null);
const minTurnoverFilter = ref(null);
const maxTurnoverFilter = ref(null);
const minMarginFilter = ref(null);

const pnlDataMap = ref({});
const loadingSet = ref(new Set()); // 用于记录正在加载的 ID

const expandedRowIds = ref(new Set());
const corrLoading = ref(false);

const getCurrentPageIDsFromDOM = () => {
  const rows = document.querySelectorAll(
    ".n-data-table-base-table-body table.n-data-table-table tbody tr"
  );
  const ids = [];

  rows.forEach((row) => {
    const idTd = row.querySelector('td[data-col-key="id"]');
    if (idTd) {
      const id = idTd.textContent?.trim();
      if (id) ids.push(id);
    }
  });
  return ids;
};

const calculateCorr = async () => {
  await nextTick(); // 等 DOM 渲染完毕

  // 获取当前页 DOM 中展示的 ID 列表
  const ids = getCurrentPageIDsFromDOM();

  // 构建当前页对应的数据对象
  const frozenPageData = ids
    .map((id) => data.value.find((item) => String(item.id) === id))
    .filter(Boolean); // 排除找不到的情况

  corrLoading.value = true;

  try {
    const result = await invoke("compute_correlation", { alphaIds: ids });

    const updatedPageData = frozenPageData.map((row) => {
      const id = String(row.id).trim();
      const corr = result[id];

      if (corr) {
        return {
          ...row,
          corr_ppac: corr.ppac_correlation,
          corr_os: corr.os_correlation,
        };
      } else {
        console.warn(`No correlation found for id: '${id}'`);
        return row;
      }
    });

    // 用 updatedPageData 更新 data.value 中对应的项
    const updatedDataMap = Object.fromEntries(
      updatedPageData.map((item) => [item.id, item])
    );

    data.value = data.value.map((row) =>
      updatedDataMap[row.id] ? updatedDataMap[row.id] : row
    );
  } catch (e) {
    console.error("计算 corr 失败", e);
  } finally {
    corrLoading.value = false;
  }
};

// 确保 DOM 渲染完成后再初始化图表
const loadPNL = async (id) => {
  if (loadingSet.value.has(id) || pnlDataMap.value[id] !== undefined) return;

  loadingSet.value.add(id);
  try {
    const result = await invoke("get_pnl_by_id", { id });
    pnlDataMap.value[id] = result.pnl_series;
  } catch (e) {
    console.error("Error loading PnL:", e);
    pnlDataMap.value[id] = null;
  } finally {
    loadingSet.value.delete(id);
  }
};

const loadPNLsForCurrentPage = async () => {
  await nextTick(); // 等待 DOM 渲染完成
  const ids = getCurrentPageIDsFromDOM();

  await Promise.all(ids.map((id) => loadPNL(id)));
};

watchEffect(() => {
  loadPNLsForCurrentPage();
});

// 设置图表的选项
function getPNLOption(series) {
  const dates = series.map((p) => p.date);
  const pnlValues = series.map((p) => p.pnl);
  const riskNeutralizedPnL = series.map((p) => p.risk_neutralized_pnl);

  return {
    grid: {
      left: 50,
      right: 20,
      top: 30,
      bottom: 40,
      containLabel: true, // ✅ 防止轴线/label 被挤压
    },
    tooltip: { trigger: "axis" },
    legend: { show: true },
    xAxis: {
      type: "category",
      data: dates,
      position: "bottom", // ✅ 始终在下方
      boundaryGap: false, // ✅ 紧贴边缘
      axisLine: {
        onZero: false, // ✅ 不随着 y=0 移动
        lineStyle: { color: "#ccc" },
      },
      axisLabel: {
        rotate: 0,
        fontSize: 10,
        formatter: (value) => value.slice(0, 4), // 只显示年份
      },
    },
    yAxis: {
      type: "value",
      min: "dataMin", // ✅ 自动以数据最小值起点
      max: "dataMax", // ✅ 自动以数据最大值终点
      axisLabel: {
        fontSize: 10,
        formatter: (value) => {
          const absVal = Math.abs(value);
          if (absVal >= 1e7) return (value / 1e6).toFixed(1) + "M";
          if (absVal >= 1e4) return (value / 1e3).toFixed(1) + "K";
          return value.toFixed(1);
        },
      },
      splitLine: {
        lineStyle: { color: "#eee" }, // 可选：分割线样式
      },
    },
    series: [
      {
        name: "PnL",
        data: pnlValues,
        type: "line",
        symbol: "none",
        lineStyle: { color: "#81CDD6", width: 1.5 },
      },
      {
        name: "Risk Neutralized PnL",
        data: riskNeutralizedPnL,
        type: "line",
        symbol: "none",
        lineStyle: { color: "#7CC873", width: 1.5 },
      },
    ],
  };
}

let requestCounter = 0; // 全局递增标识
const latestRequestId = ref(0); // 响应时校验使用

async function fetchData() {
  const currentRequestId = ++requestCounter;
  latestRequestId.value = currentRequestId;

  const params = {
    query: searchQuery.value.trim() || null,
    status: statusFilter.value || null,
    region: regionFilter.value || null,
    days_within: daysFilter.value ? parseInt(daysFilter.value) : null,
    min_turnover:
      minTurnoverFilter.value != null ? parseFloat(minTurnoverFilter.value) / 100 : null,
    max_turnover:
      maxTurnoverFilter.value != null ? parseFloat(maxTurnoverFilter.value) / 100 : null,
    min_margin:
      minMarginFilter.value != null ? parseFloat(minMarginFilter.value) / 10000 : null,
  };

  try {
    const result = await invoke("get_alpha_results", { params });

    // ⚠️ 只更新最新请求的结果
    if (currentRequestId === latestRequestId.value) {
      data.value = result;
    } else {
      console.log("⚠️ Stale response ignored");
    }
  } catch (e) {
    console.error("Error fetching data:", e);
  }
}

onMounted(fetchData);

// 数据表格列配置
const columns = [
  {
    title: "Regular",
    key: "code",
    render(row) {
      const isExpanded = expandedRowIds.value.has(row.id);
      return h(
        "div",
        {
          class: ["regular-cell", isExpanded ? "expanded" : ""],
          onClick: () => {
            if (isExpanded) {
              expandedRowIds.value.delete(row.id);
            } else {
              expandedRowIds.value.add(row.id);
            }
          },
          style: {
            cursor: "pointer",
          },
        },
        row.code || "--"
      );
    },
  },
  { title: "ID", key: "id" },
  { title: "Region", key: "region" },
  { title: "Sharpe", key: "sharpe", sorter: "default" },
  { title: "Fitness", key: "fitness", sorter: "default" },
  {
    title: "Returns",
    key: "returns",
    sorter: "default",
    render(row) {
      const val = row.returns;
      return val != null ? (val * 100).toFixed(2) + "%" : "--";
    },
  },
  {
    title: "Turnover",
    key: "turnover",
    sorter: "default",
    render(row) {
      const val = row.turnover;
      return val != null ? (val * 100).toFixed(2) + "%" : "--";
    },
  },
  {
    title: "Margin",
    key: "margin",
    sorter: "default",
    render(row) {
      const val = row.margin;
      return val != null ? (val * 10000).toFixed(2) + "‱" : "--";
    },
  },
  {
    title: "Sub-U Sharpe",
    key: "sub_universe_sharpe",
    sorter: "default",
    width: 70,
  },
  {
    title: "Date Created",
    key: "date_created",
    render(row) {
      return row.date_created?.split("T")[0] ?? "--";
    },
    sorter: "default",
  },
  { title: "Status", key: "status" },
  {
    title: "Message",
    key: "message",
    render(row) {
      const isExpanded = expandedRowIds.value.has("msg-" + row.id);
      return h(
        "div",
        {
          class: ["message-cell", isExpanded ? "expanded" : ""],
          onClick: () => {
            const key = "msg-" + row.id;
            if (isExpanded) {
              expandedRowIds.value.delete(key);
            } else {
              expandedRowIds.value.add(key);
            }
          },
          style: {
            cursor: "pointer",
            whiteSpace: isExpanded ? "normal" : "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            maxWidth: "300px", // 控制展开前的宽度（可调整）
          },
        },
        row.message || "--"
      );
    },
  },
  {
    title: "PnL",
    key: "pnl",
    width: 100,
    render(row) {
      return h(
        NPopover,
        {
          trigger: "hover", // 使用 click 触发 PnL 加载
          width: 600,
        },
        {
          trigger: () => {
            // 只在第一次点击时加载
            if (!pnlDataMap.value[row.id] && !loadingSet.value.has(row.id)) {
              loadPNL(row.id); // 手动触发加载
            }
            return h("span", { style: "cursor: pointer; color: #3b82f6" }, "📈 查看");
          },
          default: () => {
            // 显示加载中的状态或 PnL 图表
            if (!pnlDataMap.value[row.id]) {
              return h("div", "Loading...");
            }

            // 如果数据加载失败，显示错误提示
            if (pnlDataMap.value[row.id] === null) {
              return h("div", "❌ 加载失败");
            }

            // 渲染 PnL 图表
            return h(
              "div",
              { ref: "chartContainer", style: "width: 100%; height: 400px;" },
              [h(VChart, { option: getPNLOption(pnlDataMap.value[row.id]) })]
            );
          },
        }
      );
    },
  },
  {
    title: "Corr ppac",
    key: "corr_ppac",
    render(row) {
      return row.corr_ppac !== undefined ? row.corr_ppac : "-";
    },
  },
  {
    title: "Corr os",
    key: "corr_os",
    render(row) {
      return row.corr_os !== undefined ? row.corr_os : "-";
    },
  },
];
</script>

<style>
/* Increase specificity to ensure styles are applied */
.n-data-table.custom-table {
  --n-font-size: 12px !important; /* Use a more appropriate font size */
}

.n-data-table.custom-table th,
.n-data-table.custom-table td {
  padding: 0px 0px; /* Optional: adjust padding for better spacing */
}

.n-data-table.custom-table .n-data-table-tr--expanded .n-data-table-td {
  height: auto !important;
  white-space: normal !important;
  overflow: visible !important;
}

/* 保证展开行内容的高度自适应 */
.n-data-table.custom-table .n-data-table-tr--expanded .n-data-table-td {
  height: auto !important; /* 自适应内容高度 */
  white-space: normal !important; /* 使内容显示完全 */
  overflow: visible !important; /* 显示所有内容 */
}

/* 展开行的默认样式 */
.n-data-table-tr--expanded {
  display: table-row !important; /* 确保展开行可见 */
}

/* 限制表头高度最多两行，并显示省略号 */
.custom-table th .n-data-table-th__title {
  max-height: 2.6em; /* 控制最多两行（行高 * 2） */
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2; /* 限制最多两行 */
  -webkit-box-orient: vertical;
  line-height: 1.3em; /* 行高，适配 max-height */
  font-size: 12px; /* 可自定义字体大小 */
  white-space: normal !important;
  word-break: break-word;
}

/* 默认显示为单行省略号 */
.regular-cell {
  max-width: 300px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

/* 展开时显示全部内容，支持换行 */
.regular-cell.expanded {
  white-space: normal !important;
  word-break: break-word;
  overflow: visible;
}

.message-cell.expanded {
  white-space: normal !important;
  word-break: break-word;
  overflow: visible;
}


/* 确保展开后的内容能够正常显示 */
.custom-table .n-data-table-wrapper .n-data-table-table .n-data-table-td {
  max-width: 300px !important; /* 控制最大宽度 */
  height: 25px !important; /* 默认最小高度 */
  min-height: 25px !important;
  overflow: hidden !important;
  text-overflow: ellipsis !important;
  white-space: nowrap !important;
  word-wrap: break-word !important;
}

/* Apply styles directly to table cells */
/* 更具体的选择器来确保覆盖样式 */
.custom-table .n-data-table-wrapper .n-data-table-table .n-data-table-td {
  max-width: 300px !important;
  height: 25px !important;
  min-height: 25px !important;
  overflow: hidden !important;
  text-overflow: ellipsis !important;
  white-space: nowrap !important;
  position: relative !important;
  word-wrap: break-word !important;
}

/* Adjust the font size at the root level of the table */
.custom-table {
  --n-font-size: 12px !important;
}

/* On hover, show full content */
.custom-table .n-td:hover {
  white-space: normal;
  word-break: break-word;
  background-color: #f5f5f5; /* Optional: highlight on hover */
  z-index: 10;
}
</style>
