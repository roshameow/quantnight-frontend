<template>
  <div class="dataset-panel-container">
    <!-- Top Persistent JOINT Filters -->
    <div style="background: #f8f8f8; padding: 12px 16px; border-radius: 8px; margin-bottom: 12px; display: flex; align-items: center; gap: 16px; flex-wrap: wrap;">
      <div style="display: flex; align-items: center; gap: 8px; padding: 4px 8px; border: 1px dashed #ccc; border-radius: 4px; background: rgba(0,0,0,0.02)">
        <span style="font-size: 11px; color: #888; font-weight: bold; writing-mode: vertical-lr; transform: rotate(180deg); padding: 2px 0;">JOINT</span>
        <div style="width: 110px;">
          <n-select
            v-model:value="filters.selectedRegion"
            :options="regionOptions"
            placeholder="Region"
            clearable
            size="small"
            :virtual-scroll="false"
          />
        </div>
        <div style="width: 130px;">
          <n-input v-model:value="filters.filterUniverse" placeholder="Universe" clearable size="small" />
        </div>
        <div style="width: 90px;">
          <n-select
            v-model:value="filters.filterDelay"
            :options="delayOptions"
            placeholder="Delay"
            clearable
            size="small"
            :virtual-scroll="false"
          />
        </div>
      </div>

      <!-- Quick stats or info can go here if needed -->
      <div v-if="viewMode === 'datafields'" style="display: flex; align-items: center; gap: 8px; border-left: 1px solid #ddd; padding-left: 16px; overflow: hidden">
        <n-button size="small" @click="backToDatasets">
          <template #icon><span>←</span></template>
          Back
        </n-button>
        <div style="display: flex; align-items: center; gap: 8px; overflow: hidden">
          <n-text strong style="font-size: 14px; white-space: nowrap">{{ currentDataset?.name }}</n-text>
          <n-text depth="3" style="font-size: 11px; white-space: nowrap">({{ currentDataset?.id }})</n-text>
        </div>
      </div>
    </div>

    <!-- Dataset View -->
    <div v-if="viewMode === 'datasets'">
      <div class="header">
        <div style="background: #f8f8f8; padding: 16px; border-radius: 8px; margin-bottom: 16px">
          <n-space vertical size="medium">
            <!-- Row: General Search -->
            <div style="display: flex; align-items: center; gap: 12px;">
              <div style="flex: 1;">
                <n-input
                  v-model:value="filters.searchText"
                  placeholder="General Search (Name, ID, Description)"
                  clearable
                  size="small"
                />
              </div>
            </div>

            <!-- Row: Specific Filters -->
            <div style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap;">
              <div style="width: 120px;">
                <n-input v-model:value="filters.filterId" placeholder="ID" clearable size="small" />
              </div>
              <div style="width: 120px;">
                <n-input v-model:value="filters.filterFieldId" placeholder="Field ID" clearable size="small" />
              </div>
              <div style="width: 150px;">
                <n-input v-model:value="filters.filterName" placeholder="Name" clearable size="small" />
              </div>
              <div style="width: 130px;">
                <n-select
                  v-model:value="filters.filterCategory"
                  :options="categoryOptions"
                  placeholder="Category"
                  clearable
                  size="small"
                  :virtual-scroll="false"
                />
              </div>
            </div>
          </n-space>
        </div>
      </div>

      <n-data-table
        remote
        :columns="datasetColumns"
        :data="datasets"
        :loading="loading"
        :bordered="false"
        size="small"
        class="custom-table"
        :row-key="(row) => row.id"
        max-height="calc(100vh - 350px)"
        :pagination="pagination"
        @update:page="handlePageChange"
        @update:page-size="handlePageSizeChange"
        @update:sorter="handleSorterChange"
        :row-props="datasetRowProps"
      />
    </div>

    <!-- Datafield View -->
    <div v-else-if="viewMode === 'datafields'">
      <div class="header">
        <div style="background: #f8f8f8; padding: 12px 16px; border-radius: 8px; margin-bottom: 16px">
          <div style="display: flex; align-items: center; gap: 16px;">
            <!-- 搜索字段 -->
            <div style="flex: 1;">
              <n-input
                v-model:value="fieldSearchText"
                placeholder="Search Field ID or Description"
                clearable
                size="small"
              />
            </div>
          </div>
        </div>
      </div>

      <n-data-table
        remote
        :columns="datafieldColumns"
        :data="datafields"
        :loading="loading"
        :bordered="false"
        size="small"
        class="custom-table"
        :row-key="(row) => row.id"
        max-height="calc(100vh - 300px)"
        :pagination="fieldPagination"
        @update:page="handleFieldPageChange"
        @update:page-size="handleFieldPageSizeChange"
        @update:sorter="handleFieldSorterChange"
      />
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, computed, h, reactive, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { NButton, NSelect, NDataTable, NInput, NTag, NTooltip, NText, NSpace } from 'naive-ui';
import '../assets/table-styles.css';

const loading = ref(false);
const viewMode = ref('datasets'); // 'datasets' or 'datafields'
const datasets = ref([]);
const currentDataset = ref(null);
const datafields = ref([]);

const filters = reactive({
  searchText: '',
  filterId: '',
  filterFieldId: '',
  filterName: '',
  filterCategory: null,
  selectedRegion: null,
  filterUniverse: '',
  filterDelay: null,
});

const fieldSearchText = ref('');
const currentSorter = ref(null);
const currentFieldSorter = ref(null);
const expandedRowIds = ref(new Set());

const delayOptions = [
  { label: 'Delay 0', value: 0 },
  { label: 'Delay 1', value: 1 },
];

const categoryOptions = [
  'Analyst', 'Broker', 'Earnings', 'Fundamental', 'Imbalance', 'Insiders', 
  'Institutions', 'Macro', 'Model', 'News', 'Option', 'Other', 
  'Price Volume', 'Risk', 'Sentiment', 'Short Interest', 'Social Media'
].map(c => ({ label: c, value: c }));

const isMatchingTag = (d) => {
  if (filters.selectedRegion && d.region !== filters.selectedRegion) return false;
  if (filters.filterUniverse && !d.universe.toLowerCase().includes(filters.filterUniverse.toLowerCase())) return false;
  if (filters.filterDelay !== null && d.delay !== filters.filterDelay) return false;
  
  // Return true if any filter is active and matches
  return !!(filters.selectedRegion || filters.filterUniverse || filters.filterDelay !== null);
};

// Debounced watcher for filters
let debounceTimer = null;
watch(filters, () => {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    handleSearch();
  }, 300);
}, { deep: true });

// Also watch field search
watch(fieldSearchText, () => {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    handleFieldSearch();
  }, 300);
});

// Pagination for Datasets
const pagination = reactive({
  page: 1,
  pageSize: 20,
  showSizePicker: true,
  pageSizes: [20, 50, 100, 200],
  itemCount: 0,
  prefix ({ itemCount }) {
    return `Total ${itemCount} items`;
  }
});

// Pagination for Datafields
const fieldPagination = reactive({
  page: 1,
  pageSize: 20,
  showSizePicker: true,
  pageSizes: [20, 50, 100, 200, 500],
  itemCount: 0,
  prefix ({ itemCount }) {
    return `Total ${itemCount} fields`;
  }
});

const regionOptions = [
  { label: 'USA', value: 'USA' },
  { label: 'JPN', value: 'JPN' },
  { label: 'EUR', value: 'EUR' },
  { label: 'HKG', value: 'HKG' },
  { label: 'CHN', value: 'CHN' },
  { label: 'AMR', value: 'AMR' },
  { label: 'ASI', value: 'ASI' },
  { label: 'GLB', value: 'GLB' },
  { label: 'KOR', value: 'KOR' },
  { label: 'IND', value: 'IND' },
];

const datasetColumns = [
  {
    title: 'ID',
    key: 'id',
    width: 120,
    sorter: true,
    render(row) {
      const isExpanded = expandedRowIds.value.has('id-' + row.id);
      return h(
        'div',
        {
          class: ['regular-cell', isExpanded ? 'expanded' : ''],
          style: { cursor: 'pointer', fontFamily: 'monospace', fontSize: '11px' },
          onClick: () => {
            if (window.getSelection().toString()) return;
            isExpanded ? expandedRowIds.value.delete('id-' + row.id) : expandedRowIds.value.add('id-' + row.id);
          },
        },
        row.id
      );
    }
  },
  {
    title: 'Name',
    key: 'name',
    width: 180,
    sorter: true,
    render(row) {
      const isExpanded = expandedRowIds.value.has('name-' + row.id);
      return h(
        'div',
        {
          class: ['regular-cell', isExpanded ? 'expanded' : ''],
          style: { cursor: 'pointer' },
          onClick: () => {
            if (window.getSelection().toString()) return;
            isExpanded ? expandedRowIds.value.delete('name-' + row.id) : expandedRowIds.value.add('name-' + row.id);
          },
        },
        [h(NText, { strong: true, style: 'font-size: 12px' }, { default: () => row.name })]
      );
    }
  },
  {
    title: 'Category',
    key: 'category',
    width: 150,
    render(row) {
      const isExpanded = expandedRowIds.value.has('cat-' + row.id);
      return h(
        'div',
        {
          class: ['regular-cell', isExpanded ? 'expanded' : ''],
          style: { cursor: 'pointer' },
          onClick: () => {
            if (window.getSelection().toString()) return;
            isExpanded ? expandedRowIds.value.delete('cat-' + row.id) : expandedRowIds.value.add('cat-' + row.id);
          },
        },
        row.category?.name || '--'
      );
    }
  },
  {
    title: 'Description',
    key: 'description',
    minWidth: 250, // Fluid column to take up remaining space
    render(row) {
      const isExpanded = expandedRowIds.value.has(row.id);
      return h(
        'div',
        {
          class: ['regular-cell', 'no-max-width', isExpanded ? 'expanded' : ''],
          style: { cursor: 'pointer' },
          onClick: (e) => {
            e.stopPropagation();
            if (window.getSelection().toString()) return;
            isExpanded ? expandedRowIds.value.delete(row.id) : expandedRowIds.value.add(row.id);
          },
        },
        row.description || '--'
      );
    }
  },
  {
    title: 'Fields',
    key: 'totalFieldCount',
    width: 70,
    align: 'right',
    sorter: true,
    render(row) {
      const sum = row.data.reduce((acc, d) => acc + (d.fieldCount || 0), 0);
      return h('div', { style: 'font-size: 11px' }, sum > 0 ? sum : '--');
    }
  },
  {
    title: 'Alphas',
    key: 'totalAlphaCount',
    width: 90,
    align: 'right',
    sorter: true,
    render(row) {
      const sum = row.data.reduce((acc, d) => acc + (d.alphaCount || 0), 0);
      return h('div', { style: { fontSize: '11px', paddingRight: '20px' } }, sum > 0 ? sum : '--');
    }
  },
  {
    title: 'Regions',
    key: 'regions',
    width: 200,
    render(row) {
      const isExpanded = expandedRowIds.value.has('reg-' + row.id);
      // Don't filter the tags being displayed - show all regions the dataset has
      const displayData = row.data;

      return h(
        'div', 
        { 
          class: ['regular-cell', isExpanded ? 'expanded' : ''],
          style: { 
            display: 'flex', 
            flexWrap: isExpanded ? 'wrap' : 'nowrap', 
            gap: '4px', 
            overflow: 'hidden',
            cursor: 'pointer',
            paddingLeft: '12px' // Add gap from previous column
          },
          onClick: () => {
            isExpanded ? expandedRowIds.value.delete('reg-' + row.id) : expandedRowIds.value.add('reg-' + row.id);
          }
        }, 
        displayData.map(d => {
          const content = h('div', { style: 'padding: 4px; font-size: 12px' }, [
            h('div', `Region: ${d.region}`),
            h('div', `Delay: ${d.delay}`),
            h('div', `Universe: ${d.universe}`),
            d.coverage ? h('div', `Coverage: ${(d.coverage * 100).toFixed(2)}%`) : null,
            d.fieldCount ? h('div', `Field Count: ${d.fieldCount}`) : null,
            d.alphaCount ? h('div', `Alpha Count: ${d.alphaCount}`) : null,
            d.userCount !== undefined ? h('div', `User Count: ${d.userCount}`) : null,
          ]);

          return h(NTooltip, { trigger: 'hover', placement: 'top' }, {
            trigger: () => h(NTag, { 
              size: 'tiny', 
              // Highlight the region tag only if it matches all active filters (Region/Universe/Delay)
              type: isMatchingTag(d) ? 'success' : 'default',
              style: 'cursor: help; margin: 0'
            }, { 
              default: () => `${d.region}` 
            }),
            default: () => content
          });
        })
      );
    }
  },
  {
    title: 'Action',
    key: 'actions',
    width: 70,
    render(row) {
      return h(
        NButton,
        {
          size: 'tiny',
          secondary: true,
          type: 'primary',
          onClick: () => enterDataset(row)
        },
        { default: () => 'Enter' }
      );
    }
  }
];

const datafieldColumns = [
  {
    title: 'Field ID',
    key: 'id',
    width: 200,
    render(row) {
      const isExpanded = expandedRowIds.value.has('fid-' + row.id);
      return h(
        'div',
        {
          class: ['regular-cell', isExpanded ? 'expanded' : ''],
          style: { cursor: 'pointer', fontFamily: 'monospace', fontSize: '11px', fontWeight: 'bold' },
          onClick: () => {
            if (window.getSelection().toString()) return;
            isExpanded ? expandedRowIds.value.delete('fid-' + row.id) : expandedRowIds.value.add('fid-' + row.id);
          },
        },
        row.id
      );
    }
  },
  {
    title: 'Type',
    key: 'type',
    width: 80,
    render(row) {
      return h(NTag, { size: 'tiny', type: 'info', ghost: true }, { default: () => row.type || '--' });
    }
  },
  {
    title: 'Coverage',
    key: 'coverage',
    width: 90,
    align: 'right',
    sorter: true,
    render(row) {
      // Find data matching current JOINT filters
      const matchingData = row.data.find(d => isMatchingTag(d));
      if (matchingData && matchingData.coverage != null) {
        return h('div', { style: 'font-size: 11px; font-weight: bold; color: #18a058' }, `${(matchingData.coverage * 100).toFixed(2)}%`);
      }
      // Fallback: show max coverage if no filter or no match
      const maxCov = Math.max(...row.data.map(d => d.coverage || 0));
      return h('div', { style: 'font-size: 11px; color: #999' }, maxCov > 0 ? `${(maxCov * 100).toFixed(2)}%` : '--');
    }
  },
  {
    title: 'Date Coverage',
    key: 'dateCoverage',
    width: 100,
    align: 'right',
    render(row) {
      return h('div', { 
        style: row.dateCoverage != null ? 'font-size: 11px; color: #18a058' : 'font-size: 11px; color: #999' 
      }, row.dateCoverage != null ? `${(row.dateCoverage * 100).toFixed(2)}%` : '--');
    }
  },
  {
    title: 'Alphas',
    key: 'alphaCount',
    width: 70,
    align: 'right',
    sorter: true,
    render(row) {
      const matchingData = row.data.find(d => isMatchingTag(d));
      if (matchingData && matchingData.alphaCount != null) {
        return h('div', { style: 'font-size: 11px; font-weight: bold; color: #18a058' }, matchingData.alphaCount);
      }
      const sum = row.data.reduce((acc, d) => acc + (d.alphaCount || 0), 0);
      return h('div', { style: 'font-size: 11px; color: #999' }, sum > 0 ? sum : '--');
    }
  },
  {
    title: 'Description',
    key: 'description',
    render(row) {
      const isExpanded = expandedRowIds.value.has(row.id);
      return h(
        'div',
        {
          class: ['regular-cell', 'no-max-width', isExpanded ? 'expanded' : ''],
          style: { cursor: 'pointer' },
          onClick: () => {
            if (window.getSelection().toString()) return;
            isExpanded ? expandedRowIds.value.delete(row.id) : expandedRowIds.value.add(row.id);
          },
        },
        row.description || '--'
      );
    }
  },
  {
    title: 'Region Coverage',
    key: 'regions',
    width: 300,
    render(row) {
      const isExpanded = expandedRowIds.value.has('reg-' + row.id);
      return h(
        'div', 
        { 
          class: ['regular-cell', isExpanded ? 'expanded' : ''],
          style: { 
            display: 'flex', 
            flexWrap: isExpanded ? 'wrap' : 'nowrap', 
            gap: '4px', 
            overflow: 'hidden',
            cursor: 'pointer'
          },
          onClick: () => {
            isExpanded ? expandedRowIds.value.delete('reg-' + row.id) : expandedRowIds.value.add('reg-' + row.id);
          }
        }, 
        row.data.map(d => {
          const content = h('div', { style: 'padding: 4px; font-size: 12px' }, [
            h('div', `Region: ${d.region}`),
            h('div', `Delay: ${d.delay}`),
            h('div', `Universe: ${d.universe}`),
            d.coverage ? h('div', `Coverage: ${(d.coverage * 100).toFixed(2)}%`) : null,
            d.alphaCount ? h('div', `Alpha Count: ${d.alphaCount}`) : null,
            d.userCount !== undefined ? h('div', `User Count: ${d.userCount}`) : null,
          ]);

          return h(NTooltip, { trigger: 'hover', placement: 'top' }, {
            trigger: () => h(NTag, { 
              size: 'tiny', 
              // Highlight the matching tag in datafield view too
              type: isMatchingTag(d) ? 'success' : 'default',
              style: 'cursor: help; margin: 0'
            }, { 
              default: () => `${d.region}` 
            }),
            default: () => content
          });
        })
      );
    }
  }
];

const datasetRowProps = (row) => {
  return {};
};

async function fetchDatasets() {
  loading.value = true;
  try {
    const params = {
      page: pagination.page,
      page_size: pagination.pageSize,
      search: filters.searchText.trim() || null,
      id: filters.filterId.trim() || null,
      name: filters.filterName.trim() || null,
      field_id: filters.filterFieldId.trim() || null,
      category: filters.filterCategory || null,
      region: filters.selectedRegion || null,
      universe: filters.filterUniverse.trim() || null,
      delay: filters.filterDelay !== null ? filters.filterDelay : null,
      sort_field: currentSorter.value?.columnKey || null,
      sort_order: currentSorter.value?.order === 'descend' ? -1 : (currentSorter.value?.order === 'ascend' ? 1 : null)
    };
    
    const result = await invoke('get_datasets', { params });
    datasets.value = result.data;
    pagination.itemCount = result.total;
  } catch (e) {
    console.error('Failed to fetch datasets:', e);
  } finally {
    loading.value = false;
  }
}

async function fetchDatafields() {
  if (!currentDataset.value) return;
  loading.value = true;
  try {
    const params = {
      dataset_id: currentDataset.value.id,
      page: fieldPagination.page,
      page_size: fieldPagination.pageSize,
      search: fieldSearchText.value.trim() || null,
      region: filters.selectedRegion || null,
      universe: filters.filterUniverse.trim() || null,
      delay: filters.filterDelay !== null ? filters.filterDelay : null,
      sort_field: currentFieldSorter.value?.columnKey || null,
      sort_order: currentFieldSorter.value?.order === 'descend' ? -1 : (currentFieldSorter.value?.order === 'ascend' ? 1 : null)
    };
    
    const result = await invoke('get_datafields', { params });
    datafields.value = result.data;
    fieldPagination.itemCount = result.total;
  } catch (e) {
    console.error('Failed to fetch datafields:', e);
  } finally {
    loading.value = false;
  }
}

function enterDataset(dataset) {
  currentDataset.value = dataset;
  viewMode.value = 'datafields';
  fieldPagination.page = 1;
  fieldSearchText.value = '';
  fetchDatafields();
}

function backToDatasets() {
  viewMode.value = 'datasets';
  currentDataset.value = null;
  datafields.value = [];
}

function handlePageChange(page) {
  pagination.page = page;
  fetchDatasets();
}

function handlePageSizeChange(pageSize) {
  pagination.pageSize = pageSize;
  pagination.page = 1;
  fetchDatasets();
}

function handleSearch() {
  pagination.page = 1;
  fetchDatasets();
}

function handleSorterChange(sorter) {
  currentSorter.value = sorter;
  fetchDatasets();
}

function handleFieldPageChange(page) {
  fieldPagination.page = page;
  fetchDatafields();
}

function handleFieldPageSizeChange(pageSize) {
  fieldPagination.pageSize = pageSize;
  fieldPagination.page = 1;
  fetchDatafields();
}

function handleFieldSearch() {
  fieldPagination.page = 1;
  fetchDatafields();
}

function handleFieldSorterChange(sorter) {
  currentFieldSorter.value = sorter;
  fetchDatafields();
}

onMounted(() => {
  fetchDatasets();
});
</script>

<style scoped>
.dataset-panel-container {
  padding: 8px 0;
}
.header {
  margin-bottom: 16px;
}
.dataset-table {
  background-color: #fff;
}
</style>
