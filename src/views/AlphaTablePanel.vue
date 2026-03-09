<template>
  <div>
    <DataFilters
      :model-value="filters"
      @update:model-value="onFiltersUpdate"
      :corr-loading="corrLoading"
      @calculate-corr="calculateCorr"
    />

    <div style="background: #f8f8f8; padding: 8px 16px; border-radius: 8px; margin-bottom: 6px; display: flex; align-items: center; gap: 12px;">
      <span style="font-size: 13px; color: #666;">显示详情:</span>
      <n-checkbox-group v-model:value="visibleColumns">
        <n-space>
          <n-checkbox v-for="opt in columnOptions" :key="opt.value" :value="opt.value">
            {{ opt.label }}
          </n-checkbox>
        </n-space>
      </n-checkbox-group>
    </div>

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
import "../assets/table-styles.css";

// --- Reactive State ---
const configStore = useConfigStore();
const data = ref([]);
const corrLoading = ref(false);
const pnlDataMap = ref({});
const loadingSet = ref(new Set());
const expandedRowIds = ref(new Set());
const visibleColumns = ref(['score', 'pnl', 'message', 'corr']);

const columnOptions = [
  { label: 'Score', value: 'score' },
  { label: 'OS Stats', value: 'os' },
  { label: 'Universe', value: 'universe' },
  { label: 'Neutralization', value: 'neutralization' },
  { label: 'PnL', value: 'pnl' },
  { label: 'Message', value: 'message' },
  { label: 'Correlation', value: 'corr' },
];

const filters = reactive({
  collection: "alpha_results",
  embedding: "",
  searchQuery: "",
  id: "",
  messages: [], // This is for NIN
  messagesIn: [], // This is for IN
  classificationsIn: [], // This is for classifications IN
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
  visibleColumns,
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
    classifications_in: filters.classificationsIn.length > 0 ? filters.classificationsIn : null,
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
