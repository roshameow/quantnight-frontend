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
      <n-dropdown
        placement="bottom-start"
        trigger="manual"
        :x="xRef"
        :y="yRef"
        :options="options"
        :show="showDropdownRef"
        :on-clickoutside="onClickoutside"
        @select="handleMenuSelect"
      />
      <div v-if="rowError" role="alert" style="color: #d03050; margin-bottom: 8px">
        加载列表失败：{{ rowError }}
      </div>
      <n-data-table
        :loading="rowsLoading"
        :columns="columns"
        :data="data"
        :bordered="false"
        :scroll-x="1200"
        class="custom-table"
        :row-key="(row) => row.id"
        :row-props="rowProps"
        v-model:expanded-row-keys="expandedTreeRowKeys"
        :cascade="false"
        remote
        @update:sorter="handleSorterUpdate"
      />

      <div style="display: flex; justify-content: flex-end; align-items: center; gap: 12px; margin-top: 12px">
        <span role="status" style="font-size: 13px; color: #666">{{ totalLabel }}</span>
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
import { ref, watch, onMounted, onBeforeUnmount, reactive, nextTick, computed } from "vue";
import { useMessage } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import DataFilters from "../components/data/DataFilters.vue";
import { useAlphaTableColumns } from "../composables/useAlphaTableColumns.js";
import { createAlphaListLoader, provisionalItemCount } from "../utils/alphaListLoader.js";

import { useConfigStore } from "../stores/configStore.js";
import { useAnalysisStore } from "../stores/analysisStore.js";
import "../assets/table-styles.css";

// --- Reactive State ---
const configStore = useConfigStore();
const analysisStore = useAnalysisStore();
const message = useMessage();
const data = ref([]);
const corrLoading = ref(false);
const rowsLoading = ref(false);
const rowError = ref("");
const exactTotal = ref(null);
const countStatus = ref("pending");
const totalLabel = computed(() => countStatus.value === "exact"
  ? `共 ${exactTotal.value} 条`
  : countStatus.value === "unavailable" ? "总数暂不可用" : "总数统计中");
let isPanelMounted = false;
const pnlDataMap = ref({});
const loadingSet = ref(new Set());
const expandedRowIds = ref(new Set());
const visibleColumns = ref(['score', 'pnl', 'message', 'corr', 'corr_prod']);

// Context Menu State
const showDropdownRef = ref(false);
const xRef = ref(0);
const yRef = ref(0);
const selectedRow = ref(null);
const options = [
  {
    label: "添加到数据分析",
    key: "add-to-analysis",
  }
];

const expandedTreeRowKeys = ref([]);

const rowProps = (row) => {
  return {
    class:
      row.alpha_type === "RA_CHILD"
        ? "ra-child-row"
        : row.alpha_type === "RA_PARENT"
        ? "ra-parent-row"
        : "",
    onContextmenu: (e) => {
      e.preventDefault();
      showDropdownRef.value = false;
      nextTick().then(() => {
        showDropdownRef.value = true;
        xRef.value = e.clientX;
        yRef.value = e.clientY;
        selectedRow.value = row;
      });
    }
  };
};

const handleMenuSelect = (key) => {
  showDropdownRef.value = false;
  if (key === "add-to-analysis" && selectedRow.value) {
    const alpha = selectedRow.value;
    
    // In Pinia store with setup syntax, state properties are refs
    if (analysisStore.selectedAlphaIds.has(alpha.id)) {
      message.info(`Alpha ${alpha.id} 已在分析列表中`);
    } else {
      // Add to set and map
      // We need to trigger reactivity by creating new instances or using methods that Vue tracks
      analysisStore.selectedAlphaIds.add(alpha.id);
      analysisStore.selectedAlphasMap.set(alpha.id, alpha);
      
      message.success(`已将 Alpha ${alpha.id} 添加到数据分析`);
    }
  }
};

const onClickoutside = () => {
  showDropdownRef.value = false;
};

const columnOptions = [
  { label: 'Category', value: 'category' },
  { label: 'Settings', value: 'settings' },
  { label: 'Score', value: 'score' },
  { label: 'OS Stats', value: 'os' },
  { label: 'Drawdown', value: 'drawdown' },
  { label: 'PnL', value: 'pnl' },
  { label: 'Message', value: 'message' },
  { label: 'Correlation', value: 'corr' },
  { label: 'Corr Prod', value: 'corr_prod' },
];

const filters = reactive({
  collection: "alpha_results",
  embedding: "",
  searchQuery: "",
  excludeQuery: "",
  id: "",
  messages: [], // This is for NIN
  messagesIn: [], // This is for IN
  classificationsIn: [], // This is for classifications IN
  region: "",
  universe: "",
  delay: null,
  days: null,
  minTurnover: null,
  maxTurnover: null,
  minMargin: null,
  minReturn: null,
  type: "REGULAR",
  selfCategories: [],
});

const onFiltersUpdate = (newFilters) => {
  // Deep watchers receive the same object for old/new; compare before assignment instead.
  const collectionChanged = newFilters.collection !== filters.collection;
  Object.assign(filters, newFilters, collectionChanged ? { messages: [] } : {});
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
let deliveredRows = null;
let uiGeneration = 0;
const listLoader = createAlphaListLoader({
  invoke,
  onState: (state) => {
    if (uiGeneration !== state.generation) {
      uiGeneration = state.generation;
      pnlDataMap.value = {};
      loadingSet.value = new Set();
      corrLoading.value = false;
    }
    if (deliveredRows !== state.rows) {
      deliveredRows = state.rows;
      data.value = state.rows;
      corrLoading.value = false;
    }
    rowsLoading.value = state.loading;
    rowError.value = state.error ? String(state.error) : "";
    exactTotal.value = state.total;
    countStatus.value = state.countStatus;
    pagination.value.itemCount = state.countStatus === "exact"
      ? state.total
      : provisionalItemCount(state.page, state.pageSize, state.rows, state.hasRows);
    pagination.value.page = state.page;
    pagination.value.pageSize = state.pageSize;
  },
});

function requestParams() {
  return {
    collection: filters.collection || "alpha_results",
    embedding: filters.embedding || null,
    query: filters.searchQuery.trim() || null,
    exclude_query: filters.excludeQuery.trim() || null,
    id: filters.id.trim() || null,
    messages_nin: filters.messages.length > 0 ? filters.messages : null,
    messages_in: filters.messagesIn.length > 0 ? filters.messagesIn : null,
    classifications_in: filters.classificationsIn.length > 0 ? filters.classificationsIn : null,
    region: filters.region || null,
    universe: filters.universe || null,
    delay: filters.delay != null && filters.delay !== "" ? parseInt(filters.delay) : null,
    days_within: filters.days ? parseInt(filters.days) : null,
    min_turnover:
      filters.minTurnover != null ? parseFloat(filters.minTurnover) / 100 : null,
    max_turnover:
      filters.maxTurnover != null ? parseFloat(filters.maxTurnover) / 100 : null,
    min_margin: filters.minMargin != null ? parseFloat(filters.minMargin) / 10000 : null,
    min_returns: filters.minReturn != null ? parseFloat(filters.minReturn) / 100 : null,
    alpha_type: filters.type || null,
    self_categories: filters.selfCategories.length > 0 ? filters.selfCategories : null,
    page: pagination.value.page,
    page_size: pagination.value.pageSize,
    sort_field: sortState.value.field,
    sort_order: sortState.value.order,
  };
}

function fetchData() {
  listLoader.schedule(requestParams(), { immediate: true });
}

function scheduleData() {
  listLoader.schedule(requestParams());
}

// --- PNL & Correlation Logic ---
async function loadPNL(id) {
  if (loadingSet.value.has(id) || pnlDataMap.value[id] !== undefined) return;
  loadingSet.value.add(id);
  const generation = listLoader.getGeneration();
  const collection = filters.collection;
  const isCurrent = () => isPanelMounted && generation === listLoader.getGeneration();
  try {
    const result = await invoke("get_pnl_by_id", {
      query: { id, collection },
    });
    if (!isCurrent()) return;
    // If result has children_series (RA Parent), preserve the entire object for multi-line chart rendering
    if (result && result.children_series && result.children_series.length > 0) {
      pnlDataMap.value[id] = result;
    } else {
      pnlDataMap.value[id] = result ? result.pnl_series : null;
    }
  } catch (e) {
    if (isCurrent()) {
      console.error("Error loading PnL:", e);
      pnlDataMap.value[id] = null;
    }
  } finally {
    if (isCurrent()) loadingSet.value.delete(id);
  }
}

async function calculateCorr() {
  const generation = listLoader.getGeneration();
  const rows = data.value;
  const isCurrent = () => isPanelMounted && generation === listLoader.getGeneration()
    && data.value === rows;
  corrLoading.value = true;
  try {
    const idsOnPage = [];
    for (const item of data.value) {
      idsOnPage.push(item.id);
      if (item.children) {
        for (const child of item.children) {
          idsOnPage.push(child.id);
        }
      }
    }
    if (idsOnPage.length === 0) return;

    const result = await invoke("compute_correlation", { alphaIds: idsOnPage });

    if (!isCurrent()) return;
    const updateRowCorr = (row) => {
      const corr = result[row.id];
      let updated = corr
        ? { ...row, corr_ppac: corr.ppac_correlation, corr_os: corr.os_correlation }
        : { ...row };
      if (updated.children && updated.children.length > 0) {
        updated.children = updated.children.map(updateRowCorr);
        if (row.alpha_type === "RA_PARENT") {
          const ppacs = updated.children
            .map((c) => parseFloat(c.corr_ppac))
            .filter((v) => !isNaN(v));
          const oss = updated.children
            .map((c) => parseFloat(c.corr_os))
            .filter((v) => !isNaN(v));
          if (ppacs.length > 0) {
            updated.corr_ppac = Math.max(...ppacs).toFixed(4);
          }
          if (oss.length > 0) {
            updated.corr_os = Math.max(...oss).toFixed(4);
          }
        }
      }
      return updated;
    };
    data.value = data.value.map(updateRowCorr);
  } catch (e) {
    if (isCurrent()) console.error("计算 corr 失败", e);
  } finally {
    if (isPanelMounted && generation === listLoader.getGeneration()) corrLoading.value = false;
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
  () => {
    if (!isPanelMounted) return;
    pagination.value.page = 1; // Preserve RA filter page reset.
    // Synchronous invalidation prevents old promises from landing during the debounce.
    scheduleData();
  },
  { deep: true, flush: "sync" }
);

// Embedding only changes the filter. The mount/filter scheduler is the sole request path.
watch(() => configStore.embeddingOptions, (options) => {
  if (!filters.embedding && options && options.length > 0) {
    filters.embedding = options[0].value;
  }
}, { immediate: true });

onMounted(() => {
  isPanelMounted = true;
  scheduleData();
});

onBeforeUnmount(() => {
  isPanelMounted = false;
  listLoader.dispose();
});
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

/* RA Parent and Child row styling */
.ra-parent-row > td {
  background-color: #f8faff !important;
}
.ra-parent-row:hover > td {
  background-color: #edf4ff !important;
}
.ra-child-row > td {
  background-color: #f8faf8 !important;
}
.ra-child-row:hover > td {
  background-color: #edf7ed !important;
}
</style>
