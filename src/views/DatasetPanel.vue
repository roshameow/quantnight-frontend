<template>
  <div class="dataset-panel-container">
    <!-- Dataset View -->
    <div v-if="viewMode === 'datasets'">
      <div class="header">
        <div style="background: #f8f8f8; padding: 16px; border-radius: 8px; margin-bottom: 16px">
          <div style="display: flex; align-items: center; gap: 12px;">
            <!-- 地区选择 -->
            <div style="width: 200px; flex-shrink: 0;">
              <n-select
                v-model:value="selectedRegion"
                :options="regionOptions"
                placeholder="选择地区"
                clearable
                style="width: 100%"
                @update:value="handleSearch"
              />
            </div>

            <!-- 搜索输入 -->
            <div style="flex: 1; min-width: 200px;">
              <n-input
                v-model:value="searchText"
                placeholder="搜索数据集名称或ID (回车搜索)"
                clearable
                @keyup.enter="handleSearch"
              />
            </div>

            <!-- 刷新按钮 -->
            <div style="width: 100px; flex-shrink: 0;">
              <n-button type="primary" @click="handleSearch" :loading="loading" style="width: 100%">
                查询
              </n-button>
            </div>
          </div>
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
        max-height="calc(100vh - 280px)"
        :pagination="pagination"
        @update:page="handlePageChange"
        @update:page-size="handlePageSizeChange"
        :row-props="datasetRowProps"
      />
    </div>

    <!-- Datafield View -->
    <div v-else-if="viewMode === 'datafields'">
      <div class="header">
        <div style="background: #f8f8f8; padding: 12px 16px; border-radius: 8px; margin-bottom: 16px">
          <div style="display: flex; align-items: center; gap: 16px;">
            <n-button size="small" @click="backToDatasets">
              <template #icon>
                <span>←</span>
              </template>
              返回
            </n-button>
            
            <div style="flex: 1; display: flex; align-items: center; gap: 8px; overflow: hidden">
              <n-text strong style="font-size: 16px; white-space: nowrap">{{ currentDataset?.name }}</n-text>
              <n-text depth="3" style="font-size: 12px; white-space: nowrap">({{ currentDataset?.id }})</n-text>
            </div>

            <!-- 搜索字段 -->
            <div style="width: 250px;">
              <n-input
                v-model:value="fieldSearchText"
                placeholder="搜索字段 ID 或描述"
                clearable
                size="small"
                @keyup.enter="handleFieldSearch"
              />
            </div>
            
            <n-button type="primary" size="small" @click="handleFieldSearch" :loading="loading">
              搜索字段
            </n-button>
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
        max-height="calc(100vh - 250px)"
        :pagination="fieldPagination"
        @update:page="handleFieldPageChange"
        @update:page-size="handleFieldPageSizeChange"
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

const selectedRegion = ref(null);
const searchText = ref('');
const fieldSearchText = ref('');

// Pagination for Datasets
const pagination = reactive({
  page: 1,
  pageSize: 50,
  showSizePicker: true,
  pageSizes: [20, 50, 100, 200],
  itemCount: 0,
  prefix ({ itemCount }) {
    return `共 ${itemCount} 个数据集`;
  }
});

// Pagination for Datafields
const fieldPagination = reactive({
  page: 1,
  pageSize: 50,
  showSizePicker: true,
  pageSizes: [50, 100, 200, 500],
  itemCount: 0,
  prefix ({ itemCount }) {
    return `共 ${itemCount} 个字段`;
  }
});

const regionOptions = [
  { label: 'USA (美国)', value: 'USA' },
  { label: 'JPN (日本)', value: 'JPN' },
  { label: 'EUR (欧洲)', value: 'EUR' },
  { label: 'HKG (香港)', value: 'HKG' },
  { label: 'CHN (中国)', value: 'CHN' },
  { label: 'AMR (美洲)', value: 'AMR' },
  { label: 'ASI (亚洲)', value: 'ASI' },
  { label: 'GLB (全球)', value: 'GLB' },
  { label: 'KOR (韩国)', value: 'KOR' },
  { label: 'IND (印度)', value: 'IND' },
];

const datasetColumns = [
  {
    title: 'ID',
    key: 'id',
    width: 120,
    render(row) {
      return h('div', { class: 'regular-cell', style: 'font-family: monospace; font-size: 11px' }, row.id);
    }
  },
  {
    title: '名称',
    key: 'name',
    width: 180,
    render(row) {
      return h('div', { class: 'regular-cell' }, [
        h(NText, { strong: true, style: 'font-size: 12px' }, { default: () => row.name })
      ]);
    }
  },
  {
    title: '分类',
    key: 'category',
    width: 100,
    render(row) {
      return h('div', { class: 'regular-cell' }, row.category?.name || '--');
    }
  },
  {
    title: '可用地区',
    key: 'regions',
    width: 300,
    render(row) {
      const displayData = selectedRegion.value 
        ? row.data.filter(d => d.region === selectedRegion.value)
        : row.data;

      return h('div', { style: 'display: flex; flex-wrap: nowrap; gap: 4px; overflow: hidden' }, 
        displayData.map(d => {
          const content = h('div', { style: 'padding: 4px; font-size: 12px' }, [
            h('div', `地区: ${d.region}`),
            h('div', `延时: ${d.delay}`),
            h('div', `股票池: ${d.universe}`),
            d.coverage ? h('div', `覆盖度: ${(d.coverage * 100).toFixed(2)}%`) : null,
            d.fieldCount ? h('div', `字段数: ${d.fieldCount}`) : null,
            d.alphaCount ? h('div', `Alpha数: ${d.alphaCount}`) : null,
          ]);

          return h(NTooltip, { trigger: 'hover', placement: 'top' }, {
            trigger: () => h(NTag, { 
              size: 'tiny', 
              type: d.region === selectedRegion.value ? 'success' : 'default',
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
    title: '描述',
    key: 'description',
    render(row) {
      return h('div', { class: 'regular-cell', style: 'font-size: 11px' }, row.description);
    }
  }
];

const datafieldColumns = [
  {
    title: '字段 ID',
    key: 'id',
    width: 200,
    render(row) {
      return h('div', { class: 'regular-cell', style: 'font-family: monospace; font-size: 11px; font-weight: bold' }, row.id);
    }
  },
  {
    title: '类型',
    key: 'type',
    width: 80,
    render(row) {
      return h(NTag, { size: 'tiny', type: 'info', ghost: true }, { default: () => row.type || '--' });
    }
  },
  {
    title: '地区覆盖',
    key: 'regions',
    width: 300,
    render(row) {
      return h('div', { style: 'display: flex; flex-wrap: nowrap; gap: 4px; overflow: hidden' }, 
        row.data.map(d => {
          const content = h('div', { style: 'padding: 4px; font-size: 12px' }, [
            h('div', `地区: ${d.region}`),
            h('div', `延时: ${d.delay}`),
            h('div', `股票池: ${d.universe}`),
            d.coverage ? h('div', `覆盖度: ${(d.coverage * 100).toFixed(2)}%`) : null,
            d.alphaCount ? h('div', `Alpha数: ${d.alphaCount}`) : null,
          ]);

          return h(NTooltip, { trigger: 'hover', placement: 'top' }, {
            trigger: () => h(NTag, { 
              size: 'tiny', 
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
    title: '描述',
    key: 'description',
    render(row) {
      return h('div', { class: 'regular-cell', style: 'font-size: 11px' }, row.description);
    }
  }
];

const datasetRowProps = (row) => {
  return {
    style: 'cursor: pointer',
    onClick: () => {
      enterDataset(row);
    }
  };
};

async function fetchDatasets() {
  loading.value = true;
  try {
    const params = {
      page: pagination.page,
      page_size: pagination.pageSize,
      search: searchText.value.trim() || null,
      region: selectedRegion.value || null
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
      region: selectedRegion.value || null
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
