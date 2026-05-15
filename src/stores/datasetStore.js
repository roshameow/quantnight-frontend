import { defineStore } from 'pinia';
import { ref, reactive } from 'vue';

export const useDatasetStore = defineStore('dataset', () => {
  const viewMode = ref('datasets'); // 'datasets' or 'datafields'
  const currentDataset = ref(null);
  
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

  const pagination = reactive({
    page: 1,
    pageSize: 20,
    itemCount: 0,
  });

  const fieldPagination = reactive({
    page: 1,
    pageSize: 20,
    itemCount: 0,
  });

  function resetFilters() {
    filters.searchText = '';
    filters.filterId = '';
    filters.filterFieldId = '';
    filters.filterName = '';
    filters.filterCategory = null;
    filters.selectedRegion = null;
    filters.filterUniverse = '';
    filters.filterDelay = null;
    pagination.page = 1;
  }

  return {
    viewMode,
    currentDataset,
    filters,
    fieldSearchText,
    currentSorter,
    currentFieldSorter,
    expandedRowIds,
    pagination,
    fieldPagination,
    resetFilters
  };
});
