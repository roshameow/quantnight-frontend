import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import TOML from '@iarna/toml';

export const useConfigStore = defineStore('config', () => {
  // --- State ---
  const paths = ref({
    superTemplatePath: '',
    templatePath: '',
    priorityTemplatePath: '',
  });
  const dataFilterOptions = ref([]);
  const backendConfig = ref({}); // Will hold the parsed TOML object
  const buttonMappings = ref([]); // Will hold the button mappings
  const isLoading = ref(true);
  const error = ref(null);

  // --- Private Helper Functions ---

  /**
   * Parses the string content of config.js to extract the configuration object.
   * @param {string} content The raw string content of the file.
   */
  function _parseContent(content) {
    const json_start = content.indexOf('{');
    const json_end = content.lastIndexOf('}');
    if (json_start === -1 || json_end === -1) {
      throw new Error("Could not find a JSON object in config.js content.");
    }
    const json_str = content.substring(json_start, json_end + 1);
    return JSON.parse(json_str);
  }

  /**
   * Formats the configuration object back into the string content for config.js.
   * @param {object} configObject The configuration object.
   */
  function _formatContent(configObject) {
    const json_str = JSON.stringify(configObject, null, 2); // Pretty print
    return `export const AppConfig = ${json_str};
`;
  }

  // --- Actions ---

  /**
   * Fetches the config.js content from the Rust backend and parses it.
   */
  async function fetchFrontendConfig() {
    isLoading.value = true;
    error.value = null;
    try {
      const content = await invoke('get_config_js_content');
      const config = _parseContent(content);
      paths.value = config.paths || { superTemplatePath: '', templatePath: '', priorityTemplatePath: '' };
      dataFilterOptions.value = config.dataFilterOptions || [];
    } catch (e) {
      console.error('Failed to fetch or parse config.js:', e);
      error.value = 'Failed to load configurations. Please check src/config.js.';
      paths.value = { superTemplatePath: '', templatePath: '', priorityTemplatePath: '' };
      dataFilterOptions.value = [];
    } finally {
      isLoading.value = false;
    }
  }

  /**
   * Saves the provided configuration object to the config.js file via the Rust backend.
   * @param {object} configToSave The configuration object to save.
   */
  async function saveFrontendConfig(configToSave) {
    try {
      const newContent = _formatContent(configToSave);
      await invoke('save_config_js_content', { content: newContent });
    } catch (e) {
      console.error('Failed to save config.js:', e);
      error.value = 'Failed to save configurations to backend.';
      throw e;
    }
  }

  async function fetchBackendConfig() {
    try {
      const content = await invoke('get_config_toml_content');
      backendConfig.value = TOML.parse(content);
    } catch (e) {
      console.error('Failed to fetch or parse config.toml:', e);
      error.value = 'Failed to load backend configuration.';
      backendConfig.value = {};
    }
  }

  async function saveBackendConfig(updatedConfigObject) {
    try {
      // Create a complete config object by merging with existing config
      const completeConfig = {
        ...backendConfig.value,
        ...updatedConfigObject
      };
      
      // Ensure we preserve all sections from the original config
      if (backendConfig.value.mongodb && !updatedConfigObject.mongodb) {
        completeConfig.mongodb = backendConfig.value.mongodb;
      }
      if (backendConfig.value.python && !updatedConfigObject.python) {
        completeConfig.python = backendConfig.value.python;
      }
      if (backendConfig.value.bash && !updatedConfigObject.bash) {
        completeConfig.bash = backendConfig.value.bash;
      }
      
      console.log('Saving backend config:', completeConfig);
      const newTomlContent = TOML.stringify(completeConfig);
      await invoke('save_config_toml_content', { content: newTomlContent });
      // Update the store with the new config
      backendConfig.value = completeConfig;
    } catch (e) {
      console.error('Failed to save config.toml:', e);
      error.value = 'Failed to save backend configuration.';
      throw e;
    }
  }

  async function fetchButtonMappings() {
    try {
      const content = await invoke('get_button_mappings');
      buttonMappings.value = JSON.parse(content);
    } catch (e) {
      console.error('Failed to fetch button mappings:', e);
      error.value = 'Failed to load button mappings.';
      buttonMappings.value = [];
    }
  }

  async function saveButtonMappings(mappings) {
    try {
      await invoke('save_button_mappings', { mappings });
      // Refresh the button mappings from the store
      await fetchButtonMappings();
    } catch (e) {
      console.error('Failed to save button mappings:', e);
      error.value = 'Failed to save button mappings.';
      // Don't throw error, just log it
    }
  }

  // --- Initial Load ---
  fetchFrontendConfig();
  fetchBackendConfig();
  fetchButtonMappings();

  return {
    paths,
    dataFilterOptions,
    backendConfig,
    buttonMappings,
    isLoading,
    error,
    fetchFrontendConfig,
    saveFrontendConfig,
    fetchBackendConfig,
    fetchButtonMappings,
    saveButtonMappings,
  };
});