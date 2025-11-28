import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

export const useConfigStore = defineStore('config', () => {
  // --- State ---
  const paths = ref({
    superTemplatePath: '',
    templatePath: '',
    priorityTemplatePath: '',
  });
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
   * @param {object} pathsObject The paths configuration object.
   */
  function _formatContent(pathsObject) {
    const json_str = JSON.stringify(pathsObject, null, 2); // Pretty print
    return `export const AppConfig = ${json_str};\n`;
  }

  // --- Actions ---

  /**
   * Fetches the config.js content from the Rust backend and parses it.
   */
  async function fetchConfig() {
    isLoading.value = true;
    error.value = null;
    try {
      const content = await invoke('get_config_js_content');
      paths.value = _parseContent(content);
    } catch (e) {
      console.error('Failed to fetch or parse config.js:', e);
      error.value = 'Failed to load configurations from backend.';
    } finally {
      isLoading.value = false;
    }
  }

  /**
   * Saves the current paths configuration back to the config.js file via the Rust backend.
   */
  async function saveConfig() {
    isLoading.value = true;
    error.value = null;
    try {
      const newContent = _formatContent(paths.value);
      await invoke('save_config_js_content', { content: newContent });
    } catch (e) {
      console.error('Failed to save config.js:', e);
      error.value = 'Failed to save configurations to backend.';
      throw e; // Re-throw to let the UI component know about the failure
    } finally {
      isLoading.value = false;
    }
  }

  // --- Initial Load ---
  fetchConfig();

  return {
    paths,
    isLoading,
    error,
    fetchConfig,
    saveConfig,
  };
});