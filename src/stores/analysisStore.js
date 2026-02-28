import { defineStore } from 'pinia';
import { ref } from 'vue';

export const useAnalysisStore = defineStore('analysis', () => {
  const selectedCollection = ref("alpha_results");
  const embedding = ref("");
  const searchQuery = ref("");
  const region = ref("");
  const delay = ref("");
  const searchResults = ref([]);
  const selectedAlphaIds = ref(new Set());
  const selectedAlphasMap = ref(new Map()); // Store full alpha objects
  const pnlDataMap = ref({});
  const showSelectedOnly = ref(false);
  const showAveragePnL = ref(false);
  const chartZoomState = ref({
    xAxisStart: 0,
    xAxisEnd: 100,
    yAxisStart: 0,
    yAxisEnd: 100
  });

  function reset() {
    selectedCollection.value = "alpha_results";
    embedding.value = "";
    searchQuery.value = "";
    region.value = "";
    delay.value = "";
    searchResults.value = [];
    selectedAlphaIds.value = new Set();
    selectedAlphasMap.value = new Map();
    pnlDataMap.value = {};
    showSelectedOnly.value = false;
    showAveragePnL.value = false;
    chartZoomState.value = {
        xAxisStart: 0,
        xAxisEnd: 100,
        yAxisStart: 0,
        yAxisEnd: 100
    };
  }

  return {
    selectedCollection,
    embedding,
    searchQuery,
    region,
    delay,
    searchResults,
    selectedAlphaIds,
    selectedAlphasMap,
    pnlDataMap,
    showSelectedOnly,
    showAveragePnL,
    chartZoomState,
    reset
  };
});
