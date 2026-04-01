import { defineStore } from 'pinia';
import { ref, watch } from 'vue';

export const useQueryStore = defineStore('query', () => {
  const savedQueries = ref(JSON.parse(localStorage.getItem('savedQueries') || '[]'));

  watch(savedQueries, (newVal) => {
    localStorage.setItem('savedQueries', JSON.stringify(newVal));
  }, { deep: true });

  function addQuery(type, data) {
    let queryValue = "";
    let filters = null;

    if (type === 'analysis' || type === 'submission') {
      queryValue = typeof data === 'string' ? data.trim() : (data.searchQuery || data.query || "").trim();
      if (!queryValue || queryValue === "{}" || queryValue === "") return;
    } else {
      filters = JSON.parse(JSON.stringify(data));
      queryValue = filters.searchQuery || filters.query || "";
    }

    // 检查是否已存在
    const exists = savedQueries.value.some(q => 
      q.type === type && ( (type === 'analysis' || type === 'submission') ? q.value === queryValue : JSON.stringify(q.filters) === JSON.stringify(filters))
    );

    if (exists) return;

    const query = {
      id: Date.now(),
      label: queryValue,
      value: queryValue,
      type,
      filters: filters,
      timestamp: new Date().toISOString()
    };

    savedQueries.value.unshift(query);
    if (savedQueries.value.length > 50) {
      savedQueries.value = savedQueries.value.slice(0, 50);
    }
  }

  function removeQuery(id) {
    savedQueries.value = savedQueries.value.filter(q => q.id !== id);
  }

  function getQueriesByType(type) {
    return savedQueries.value.filter(q => q.type === type);
  }

  return {
    savedQueries,
    addQuery,
    removeQuery,
    getQueriesByType
  };
});
