import { defineStore } from 'pinia';
import { ref } from 'vue';

export const useAnalysisStore = defineStore('analysis', () => {
  const selectedCollection = ref("alpha_results");
  const searchQuery = ref("");
  const region = ref("");
  const delay = ref("");
  const searchResults = ref([]);
  const selectedAlphaIds = ref(new Set());
  const pnlDataMap = ref({});
  const showSelectedOnly = ref(false);
  const chartZoomState = ref({
    xAxisStart: 0,
    xAxisEnd: 100,
    yAxisStart: 0,
    yAxisEnd: 100
  });

  function reset() {
    selectedCollection.value = "alpha_results";
    searchQuery.value = "";
    region.value = "";
    delay.value = "";
    searchResults.value = [];
    selectedAlphaIds.value = new Set();
    pnlDataMap.value = {};
    showSelectedOnly.value = false;
    chartZoomState.value = {
        xAxisStart: 0,
        xAxisEnd: 100,
        yAxisStart: 0,
        yAxisEnd: 100
    };
  }

  return {
    selectedCollection,
    searchQuery,
    region,
    delay,
    searchResults,
    selectedAlphaIds,
    pnlDataMap,
    showSelectedOnly,
    chartZoomState,
    reset
  };
});
