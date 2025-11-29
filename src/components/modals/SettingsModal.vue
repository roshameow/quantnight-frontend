<template>
  <n-modal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="Settings"
    style="width: 600px"
    :bordered="false"
  >
    <n-spin :show="configStore.isLoading">
      <div v-if="configStore.error" class="error-message">
        {{ configStore.error }}
      </div>
      <n-form label-placement="left" label-width="auto">
        <n-h3>Template Paths</n-h3>
        <n-form-item label="Super Template Path">
          <n-input v-model:value="localPaths.superTemplatePath" />
        </n-form-item>
        <n-form-item label="Template Path">
          <n-input v-model:value="localPaths.templatePath" />
        </n-form-item>
        <n-form-item label="Priority Template Path">
          <n-input v-model:value="localPaths.priorityTemplatePath" />
        </n-form-item>

        <n-h3>Data Filter Options</n-h3>
        <div v-for="(option, index) in localDataFilterOptions" :key="index" style="display: flex; align-items-center; margin-bottom: 8px;">
          <n-input v-model:value="option.label" placeholder="Label" style="margin-right: 8px;" />
          <n-input v-model:value="option.value" placeholder="Value" style="margin-right: 8px;" />
          <n-button @click="removeOption(index)" type="error" ghost>
            Remove
          </n-button>
        </div>
        <n-button @click="addOption" type="primary" ghost style="margin-top: 8px;">
          Add Option
        </n-button>

        <n-h3>Backend Config (config.toml)</n-h3>
        <n-form-item v-for="(value, key) in localBackendConfig" :key="key" :label="key">
          <n-input v-if="typeof value === 'string'" v-model:value="localBackendConfig[key]" />
          <span v-else><i>(Non-editable value)</i></span>
        </n-form-item>
      </n-form>
    </n-spin>

    <template #footer>
      <div style="display: flex; justify-content: flex-end; gap: 8px">
        <n-button @click="$emit('update:show', false)">Cancel</n-button>
        <n-button type="primary" @click="handleSave">Save</n-button>
      </div>
    </template>
  </n-modal>
</template>

<script setup>
import { ref, watch } from 'vue';
import {
  NModal,
  NSpin,
  NForm,
  NFormItem,
  NInput,
  NH3,
  NButton,
  useMessage,
} from 'naive-ui';
import { useConfigStore } from '../../stores/configStore';

const props = defineProps({
  show: {
    type: Boolean,
    required: true,
  },
});

const emit = defineEmits(['update:show']);

const configStore = useConfigStore();
const message = useMessage();

// Local state to edit configs without directly mutating the store
const localPaths = ref({
  superTemplatePath: '',
  templatePath: '',
  priorityTemplatePath: '',
});
const localDataFilterOptions = ref([]);
const localBackendConfig = ref({});

// When the modal is shown, populate local state from the store
watch(
  () => props.show,
  (newVal) => {
    if (newVal) {
      // Deep copy to avoid direct mutation
      localPaths.value = JSON.parse(JSON.stringify(configStore.paths || {}));
      localDataFilterOptions.value = JSON.parse(JSON.stringify(configStore.dataFilterOptions || []));
      localBackendConfig.value = JSON.parse(JSON.stringify(configStore.backendConfig || {}));
    }
  }
);

function addOption() {
  localDataFilterOptions.value.push({ label: '', value: '' });
}

function removeOption(index) {
  localDataFilterOptions.value.splice(index, 1);
}

async function handleSave() {
  // --- Save Frontend Config ---
  const frontendConfig = {
    paths: localPaths.value,
    dataFilterOptions: localDataFilterOptions.value,
  };
  configStore.$patch({
    paths: frontendConfig.paths,
    dataFilterOptions: frontendConfig.dataFilterOptions,
    backendConfig: localBackendConfig.value
  });

  try {
    // Await both save operations
    await Promise.all([
      configStore.saveFrontendConfig(frontendConfig),
      configStore.saveBackendConfig(localBackendConfig.value)
    ]);
    
    message.success('Settings saved successfully! Hot reload will apply changes.');
    emit('update:show', false);
  } catch (e) {
    message.error('Failed to save settings. Check console for details.');
  }
}
</script>

<style scoped>
.error-message {
  color: red;
  margin-bottom: 16px;
}
</style>
