<template>
  <div>
    <DataFilters
      :model-value="filters"
      @update:model-value="onFiltersUpdate"
      :corr-loading="corrLoading"
      @calculate-corr="calculateCorr"
    />

    <!-- Table + Pagination Container -->
    <div style="display: flex; flex-direction: column">
      <n-data-table
        :columns="columns"
        :data="data"
        :bordered="false"
        :scroll-x="1200"
        class="custom-table"
        :row-key="(row) => row.id"
        remote
        @update:sorter="handleSorterUpdate"
      />

      <div style="display: flex; justify-content: flex-end; margin-top: 12px">
        <n-pagination
          v-model:page="pagination.page"
          v-model:page-size="pagination.pageSize"
          :item-count="pagination.itemCount"
          show-quick-jumper
          show-size-picker
          :page-sizes="[20, 50, 100]"
          @update:page="fetchData"
          @update:page-size="handlePageSizeChange"
        />
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, watch, onMounted, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import DataFilters from "../components/data/DataFilters.vue";
import { useAlphaTableColumns } from "../composables/useAlphaTableColumns.js";

import { useConfigStore } from "../stores/configStore.js";

// --- Reactive State ---
const configStore = useConfigStore();
const data = ref([]);
const corrLoading = ref(false);
const pnlDataMap = ref({});
const loadingSet = ref(new Set());
const expandedRowIds = ref(new Set());

const filters = reactive({
  collection: "alpha_results",
  embedding: "",
  searchQuery: "",
  id: "",
  messages: [], // This is for NIN
  messagesIn: [], // This is for IN
  region: "",
  delay: null,
  days: null,
  minTurnover: null,
  maxTurnover: null,
  minMargin: null,
  minReturn: null,
});

const onFiltersUpdate = (newFilters) => {
  Object.assign(filters, newFilters);
};

const pagination = ref({
  page: 1,
  pageSize: 20,
  itemCount: 0,
});

const sortState = ref({
  field: null,
  order: null,
});

// --- Composables ---
const { columns } = useAlphaTableColumns({
  expandedRowIds,
  pnlDataMap,
  loadingSet,
  loadPNL,
});

// --- Data Fetching ---
let requestCounter = 0;
const latestRequestId = ref(0);

async function fetchData() {
  const currentRequestId = ++requestCounter;
  latestRequestId.value = currentRequestId;

  const params = {
    collection: filters.collection || "alpha_results",
    embedding: filters.embedding || null,
    query: filters.searchQuery.trim() || null,
    id: filters.id.trim() || null,
    messages_nin: filters.messages.length > 0 ? filters.messages : null,
    messages_in: filters.messagesIn.length > 0 ? filters.messagesIn : null,
    region: filters.region || null,
    delay: filters.delay ? parseInt(filters.delay) : null,
    days_within: filters.days ? parseInt(filters.days) : null,
    min_turnover:
      filters.minTurnover != null ? parseFloat(filters.minTurnover) / 100 : null,
    max_turnover:
      filters.maxTurnover != null ? parseFloat(filters.maxTurnover) / 100 : null,
    min_margin: filters.minMargin != null ? parseFloat(filters.minMargin) / 10000 : null,
    min_returns: filters.minReturn != null ? parseFloat(filters.minReturn) / 100 : null,
    page: pagination.value.page,
    page_size: pagination.value.pageSize,
    sort_field: sortState.value.field,
    sort_order: sortState.value.order,
  };

  try {
    const result = await invoke("get_alpha_results", { params });
    if (currentRequestId === latestRequestId.value) {
      data.value = result.data;
      pagination.value.itemCount = result.total;
      pagination.value.page = result.page;
      pagination.value.pageSize = result.page_size;
    }
  } catch (e) {
    console.error("Error fetching data:", e);
  }
}

// --- PNL & Correlation Logic ---
async function loadPNL(id) {
  if (loadingSet.value.has(id) || pnlDataMap.value[id] !== undefined) return;
  loadingSet.value.add(id);
  try {
    const result = await invoke("get_pnl_by_id", {
      query: { id, collection: filters.collection },
    });
    pnlDataMap.value[id] = result.pnl_series;
  } catch (e) {
    console.error("Error loading PnL:", e);
    pnlDataMap.value[id] = null;
  } finally {
    loadingSet.value.delete(id);
  }
}

async function calculateCorr() {
  corrLoading.value = true;
  try {
    const idsOnPage = data.value.map((item) => item.id);
    if (idsOnPage.length === 0) return;

    const result = await invoke("compute_correlation", { alphaIds: idsOnPage });

    const updatedData = data.value.map((row) => {
      const corr = result[row.id];
      if (corr) {
        return { ...row, corr_ppac: corr.ppac_correlation, corr_os: corr.os_correlation };
      }
      return row;
    });
    data.value = updatedData;
  } catch (e) {
    console.error("计算 corr 失败", e);
  } finally {
    corrLoading.value = false;
  }
}

// --- Event Handlers ---
function handleSorterUpdate(sorter) {
  if (sorter) {
    sortState.value.field = sorter.columnKey;
    sortState.value.order = sorter.order === "ascend" ? 1 : -1;
  } else {
    sortState.value.field = null;
    sortState.value.order = null;
  }
  fetchData();
}

function handlePageSizeChange(pageSize) {
  pagination.value.pageSize = pageSize;
  pagination.value.page = 1; // Reset to first page
  fetchData();
}

// --- Watchers & Lifecycle ---
watch(
  filters,
  (newFilters, oldFilters) => {
    // Check if the collection has changed
    if (newFilters.collection !== oldFilters.collection) {
      filters.messages = []; // Reset messages when collection changes
    }
    fetchData(); // Fetch data on any filter change
  },
  { deep: true }
);

onMounted(() => {
  // Set default embedding if not set
  if (!filters.embedding && configStore.embeddingOptions.length > 0) {
    filters.embedding = configStore.embeddingOptions[0].value;
  }
  fetchData();
});

watch(() => configStore.embeddingOptions, (options) => {
  if (!filters.embedding && options && options.length > 0) {
    filters.embedding = options[0].value;
    fetchData();
  }
}, { immediate: true });
</script>

<style>
.n-input {
  --n-padding-top: 4px !important;
  --n-padding-bottom: 4px !important;
  --n-padding-left: 8px !important;
  --n-padding-right: 8px !important;
  --n-font-size: 13px !important;
}

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

.message-cell {
  font-size: 12px !important;
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

/* 
  Correct override for Naive UI's teleported select menu, based on inspection.
*/
.n-base-select-menu .n-base-select-option .n-base-select-option__content {
  font-size: 12px !important;
}

/* Make the selected tags in the filter bar more compact */
.n-base-selection-tag-wrapper .n-tag {
  height: 12px !important;
  padding-left: 4px !important;
  padding-right: 0px !important;
  padding-top: 0px;
  padding-bottom: 0px;
}
.n-base-selection-tag-wrapper .n-tag .n-tag__content {
  font-size: 8px !important;
}

/* Remove padding from the virtual list container, discovered via inspection */
.n-base-select-menu .v-vl-items {
  padding-top: 0 !important;
  padding-bottom: 0 !important;
}

/* Adjust option height to be compact and consistent for virtual list */
.n-base-select-menu .n-base-select-option {
  padding-top: 1px !important;
  padding-bottom: 1px !important;
  min-height: 18px !important; /* A specific, small height for the virtual list to calculate */
}
</style>
