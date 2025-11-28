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
      </n-form>

      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="$emit('update:show', false)">Cancel</n-button>
          <n-button type="primary" @click="handleSave">Save</n-button>
        </div>
      </template>
    </n-spin>
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

// When the modal is shown, populate local state from the store
watch(
  () => props.show,
  (newVal) => {
    if (newVal) {
      // Deep copy paths object to avoid direct mutation
      localPaths.value = JSON.parse(JSON.stringify(configStore.paths));
    }
  }
);

async function handleSave() {
  // Update the store's state
  configStore.paths = localPaths.value;

  try {
    await configStore.saveConfig();
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
