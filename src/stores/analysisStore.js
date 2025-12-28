import { defineStore } from 'pinia';
import { ref } from 'vue';

export const useAnalysisStore = defineStore('analysis', () => {
  const selectedCollection = ref("alpha_results");
  const searchQuery = ref("");
  const region = ref("");
  const delay = ref("");
  const searchResults = ref([]);
  const selectedAlphaIds = ref(new Set());
  const selectedAlphasMap = ref(new Map()); // Store full alpha objects
  const pnlDataMap = ref({});
  const pcaResults = ref([]); // Store PCA points {id, x, y, cluster}
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
    selectedAlphasMap.value = new Map();
    pnlDataMap.value = {};
    pcaResults.value = [];
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
    selectedAlphasMap,
    pnlDataMap,
    pcaResults,
    showSelectedOnly,
    chartZoomState,
    reset
  };
});
