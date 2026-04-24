<template>
  <div class="dataset-panel-container">
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
      :columns="columns"
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
    />
  </div>
</template>

<script setup>
import { ref, onMounted, computed, h, reactive, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { NButton, NSelect, NDataTable, NInput, NTag, NTooltip, NText } from 'naive-ui';
import '../assets/table-styles.css';

const loading = ref(false);
const datasets = ref([]);
const total = ref(0);
const selectedRegion = ref(null);
const searchText = ref('');

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

const columns = [
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
