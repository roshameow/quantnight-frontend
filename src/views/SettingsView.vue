<template>
  <div class="settings-container">
    <n-space vertical size="large">
      <!-- Configuration Info -->
      <n-alert type="info" title="配置文件位置" style="margin-bottom: 16px;">
        配置文件存储位置：
        <br>
        优先位置：用户数据目录
        <br>
        <strong>macOS:</strong> ~/Library/Application Support/quantnight/
        <br>
        <strong>Windows:</strong> %APPDATA%\quantnight\
        <br>
        <strong>Linux:</strong> ~/.local/share/quantnight/
        <br>
        <br>
        回退位置（如果用户数据目录不可用）：
        <br>
        <strong>macOS:</strong> .app/Contents/Resources/resources/
        <br>
        <strong>Windows:</strong> 与可执行文件同目录的 resources/
        <br>
        <strong>Linux:</strong> 与可执行文件同目录的 resources/
      </n-alert>

      <!-- Frontend Configuration (from config.js) -->
      <n-card title="Frontend Configuration (config.js)" size="small">
        <template #header-extra>
          <n-tag type="info" size="small">支持热修复</n-tag>
        </template>
        <n-card title="Template Paths" size="small" style="margin-bottom: 16px;">
          <div style="display: flex; flex-direction: column; gap: 6px;">
            <div style="display: flex; align-items: center; margin-bottom: 6px;">
              <span style="width: 140px; margin-right: 8px;">Super Template Path:</span>
              <n-input spellcheck="false" v-model:value="localPaths.superTemplatePath" spellcheck="false" size="small" style="flex: 1;" />
            </div>
            <div style="display: flex; align-items: center; margin-bottom: 6px;">
              <span style="width: 140px; margin-right: 8px;">Template Path:</span>
              <n-input spellcheck="false" v-model:value="localPaths.templatePath" spellcheck="false" size="small" style="flex: 1;" />
            </div>
            <div style="display: flex; align-items: center; margin-bottom: 6px;">
              <span style="width: 140px; margin-right: 8px;">Priority Template Path:</span>
              <n-input spellcheck="false" v-model:value="localPaths.priorityTemplatePath" spellcheck="false" size="small" style="flex: 1;" />
            </div>
          </div>
        </n-card>

        <n-card title="Data Filter Options" size="small">
          <div v-for="(option, index) in localDataFilterOptions" :key="index" style="display: flex; align-items: center; margin-bottom: 6px;">
            <n-input spellcheck="false" 
              :value="option.value" 
              @update:value="(val) => { option.value = val; option.label = val; }"
              placeholder="Value" 
              style="margin-right: 8px; flex: 1;" 
              spellcheck="false" 
              size="small" 
            />
            <n-button @click="removeOption(index)" type="error" ghost size="small">
              Remove
            </n-button>
          </div>
          <n-button @click="addOption" type="primary" ghost style="margin-top: 6px;" size="small">
            Add Option
          </n-button>
        </n-card>

        <n-card title="Embedding Options" size="small">
          <div v-for="(option, index) in localEmbeddingOptions" :key="index" style="display: flex; align-items: center; margin-bottom: 6px;">
            <n-input spellcheck="false" 
              :value="option.value" 
              @update:value="(val) => { option.value = val; option.label = val; }"
              placeholder="Value" 
              style="margin-right: 8px; flex: 1;" 
              spellcheck="false" 
              size="small" 
            />
            <n-button @click="removeEmbeddingOption(index)" type="error" ghost size="small">
              Remove
            </n-button>
          </div>
          <n-button @click="addEmbeddingOption" type="primary" ghost style="margin-top: 6px;" size="small">
            Add Option
          </n-button>
        </n-card>
      </n-card>

      <!-- Backend Configuration (from config.toml) -->
      <n-card title="Backend Configuration (config.toml)" size="small">
        <template #header-extra>
          <n-tag type="warning" size="small">修改后需要手动重启应用</n-tag>
        </template>
        <n-card title="MongoDB & Database Settings" size="small" style="margin-bottom: 16px;">
        <div style="display: flex; flex-direction: column; gap: 12px;">
          <n-h4 style="margin-bottom: 8px;">MongoDB Connection</n-h4>
          <div style="display: flex; align-items: center; margin-bottom: 6px;">
            <span style="width: 140px; margin-right: 8px;">Local URI:</span>
            <n-input spellcheck="false" v-model:value="localMongoConfig.local_uri" spellcheck="false" size="small" style="flex: 1;" />
          </div>
          <div style="display: flex; align-items: center; margin-bottom: 6px;">
            <span style="width: 140px; margin-right: 8px;">Remote URI:</span>
            <n-input spellcheck="false" v-model:value="localMongoConfig.remote_uri" spellcheck="false" size="small" style="flex: 1;" />
          </div>
          
          <n-h4 style="margin-bottom: 8px; margin-top: 8px;">Database Names</n-h4>
          <div style="display: flex; align-items: center; margin-bottom: 6px;">
            <span style="width: 140px; margin-right: 8px;">Mission DB:</span>
            <n-input spellcheck="false" v-model:value="localMongoConfig.databases.mission" spellcheck="false" size="small" style="flex: 1;" />
          </div>
          <div style="display: flex; align-items: center; margin-bottom: 6px;">
            <span style="width: 140px; margin-right: 8px;">Simulation DB:</span>
            <n-input spellcheck="false" v-model:value="localMongoConfig.databases.simulation" spellcheck="false" size="small" style="flex: 1;" />
          </div>
          <div style="display: flex; align-items: center; margin-bottom: 6px;">
            <span style="width: 140px; margin-right: 8px;">Alpha DB:</span>
            <n-input spellcheck="false" v-model:value="localMongoConfig.databases.alpha" spellcheck="false" size="small" style="flex: 1;" />
          </div>
        </div>
        </n-card>

        <n-card title="Environment Settings" size="small" style="margin-bottom: 16px;">
        <div style="display: flex; flex-direction: column; gap: 12px;">
          <div style="display: flex; align-items: center; margin-bottom: 6px;">
            <span style="width: 140px; margin-right: 8px;">Working Directory:</span>
            <n-input spellcheck="false" v-model:value="localEnvConfig.working_dir" spellcheck="false" size="small" style="flex: 1;" />
          </div>
        </div>
        </n-card>

        <n-card title="Button Script Mappings" size="small">
        <n-table :single-line="false" size="small" :scroll-x="800">
          <thead>
            <tr>
              <th style="width: 100px">Button</th>
              <th style="width: 80px">Type</th>
              <th style="width: 120px">Name</th>
              <th style="width: 200px">Command</th>
              <th style="width: 150px">Description</th>
              <th style="width: 80px">Status</th>
              <th style="width: 80px">Actions</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(mapping, index) in editableButtonMappings" :key="mapping.button_id">
              <td style="font-size: 12px;">
                <n-input spellcheck="false" v-model:value="mapping.button_label" size="tiny" spellcheck="false" />
              </td>
              <td>
                <n-select v-model:value="mapping.script_type" size="tiny" style="width: 70px;" :options="[
                  { value: 'python', label: 'Python' },
                  { value: 'bash', label: 'Bash' }
                ]" />
              </td>
              <td>
                <n-input spellcheck="false" v-model:value="mapping.script_key" size="tiny" spellcheck="false" />
              </td>
              <td>
                <n-tooltip trigger="hover" :style="{ maxWidth: '400px' }">
                  <template #trigger>
                    <n-input spellcheck="false" v-model:value="mapping.script_command" size="tiny" spellcheck="false" />
                  </template>
                  {{ mapping.script_command }}
                </n-tooltip>
              </td>
              <td style="font-size: 12px;">
                <n-input spellcheck="false" v-model:value="mapping.description" size="tiny" spellcheck="false" />
              </td>
              <td>
                <n-switch v-model:value="mapping.enabled" size="small" />
              </td>
              <td>
                <n-button @click="removeMapping(index)" type="error" size="tiny" ghost>
                  删除
                </n-button>
              </td>
            </tr>
          </tbody>
        </n-table>
        <div style="margin-top: 12px; display: flex; justify-content: space-between;">
          <n-button @click="addMapping" type="primary" size="small">
            添加映射
          </n-button>
          <n-button @click="saveButtonMappings" type="success" size="small">
            保存映射
          </n-button>
          <n-button @click="resetButtonMappings" size="small">
            重置
          </n-button>
        </div>
        </n-card>
      </n-card>

      <div style="display: flex; justify-content: flex-end; gap: 8px">
        <n-button @click="handleSave" type="primary">Save Settings</n-button>
      </div>
    </n-space>
  </div>
</template>

<script setup>
import { ref, watch, onMounted } from 'vue';
import {
  NCard,
  NSpace,
  NForm,
  NFormItem,
  NInput,
  NButton,
  NTable,
  NTag,
  NCode,
  NTooltip,
  NSelect,
  NSwitch,
  NH4,
  NAlert,
  useMessage,
} from 'naive-ui';
import { useConfigStore } from '../stores/configStore';

const configStore = useConfigStore();
const message = useMessage();

// Local state to edit configs without directly mutating the store
const localPaths = ref({
  superTemplatePath: '',
  templatePath: '',
  priorityTemplatePath: '',
});
const localDataFilterOptions = ref([]);
const localEmbeddingOptions = ref([]);
const editableButtonMappings = ref([]);
const localMongoConfig = ref({
  local_uri: '',
  remote_uri: '',
  databases: {
    mission: '',
    simulation: '',
    alpha: ''
  }
});
const localEnvConfig = ref({
  working_dir: ''
});

// Initialize local state from the store
onMounted(async () => {
  // Wait for the store to be loaded before initializing local state
  await new Promise(resolve => setTimeout(resolve, 100));
  
  // Deep copy to avoid direct mutation
  localPaths.value = JSON.parse(JSON.stringify(configStore.paths || {}));
  localDataFilterOptions.value = JSON.parse(JSON.stringify(configStore.dataFilterOptions || []));
  localEmbeddingOptions.value = JSON.parse(JSON.stringify(configStore.embeddingOptions || []));
  editableButtonMappings.value = JSON.parse(JSON.stringify(configStore.buttonMappings || []));
  
  // Initialize MongoDB and Env configs from backendConfig
  if (configStore.backendConfig.mongodb) {
    localMongoConfig.value = {
      local_uri: configStore.backendConfig.mongodb.local_uri || '',
      remote_uri: configStore.backendConfig.mongodb.remote_uri || '',
      databases: {
        mission: configStore.backendConfig.mongodb.databases?.mission || '',
        simulation: configStore.backendConfig.mongodb.databases?.simulation || '',
        alpha: configStore.backendConfig.mongodb.databases?.alpha || ''
      }
    };
  }
  
  if (configStore.backendConfig.env) {
    localEnvConfig.value = {
      working_dir: configStore.backendConfig.env.working_dir || ''
    };
  }
  
  // Debug logging for initialization
  console.log('Initialized localMongoConfig:', localMongoConfig.value);
  console.log('Initialized localEnvConfig:', localEnvConfig.value);
});

function addOption() {
  localDataFilterOptions.value.push({ label: '', value: '' });
}

function removeOption(index) {
  localDataFilterOptions.value.splice(index, 1);
}

function addEmbeddingOption() {
  localEmbeddingOptions.value.push({ label: '', value: '' });
}

function removeEmbeddingOption(index) {
  localEmbeddingOptions.value.splice(index, 1);
}

async function handleSave() {
  const savePromises = [];
  
  // --- Check if Frontend Config was modified ---
  const frontendConfig = {
    paths: localPaths.value,
    dataFilterOptions: localDataFilterOptions.value,
    embeddingOptions: localEmbeddingOptions.value,
  };
  
  const frontendChanged = JSON.stringify(frontendConfig.paths) !== JSON.stringify(configStore.paths || {}) ||
                          JSON.stringify(frontendConfig.dataFilterOptions) !== JSON.stringify(configStore.dataFilterOptions || []) ||
                          JSON.stringify(frontendConfig.embeddingOptions) !== JSON.stringify(configStore.embeddingOptions || []);
  
  if (frontendChanged) {
    configStore.$patch({
      paths: frontendConfig.paths,
      dataFilterOptions: frontendConfig.dataFilterOptions,
      embeddingOptions: frontendConfig.embeddingOptions,
    });
    savePromises.push(configStore.saveFrontendConfig(frontendConfig));
  }

  // --- Check if Backend Config was modified ---
  // Only check if any backend fields have non-empty values that differ from defaults
  const mongoDbChanged = localMongoConfig.value.local_uri !== (configStore.backendConfig.mongodb?.local_uri || '') ||
                          localMongoConfig.value.remote_uri !== (configStore.backendConfig.mongodb?.remote_uri || '') ||
                          localMongoConfig.value.databases.mission !== (configStore.backendConfig.mongodb?.databases?.mission || '') ||
                          localMongoConfig.value.databases.simulation !== (configStore.backendConfig.mongodb?.databases?.simulation || '') ||
                          localMongoConfig.value.databases.alpha !== (configStore.backendConfig.mongodb?.databases?.alpha || '');
                          
  const envChanged = localEnvConfig.value.working_dir !== (configStore.backendConfig.env?.working_dir || '');

  const backendChanged = mongoDbChanged || envChanged;
  
  console.log('mongoDbChanged:', mongoDbChanged);
  console.log('envChanged:', envChanged);
  console.log('backendChanged result:', backendChanged);
  
  if (backendChanged) {
    const backendConfig = {
      mongodb: localMongoConfig.value,
      env: localEnvConfig.value
    };
    savePromises.push(configStore.saveBackendConfig(backendConfig));
  }

  try {
    if (savePromises.length === 0) {
      message.info('No changes detected.');
      return;
    }
    
    // Debug logging
    console.log('Frontend changed:', frontendChanged);
    console.log('Backend changed:', backendChanged);
    console.log('Save promises:', savePromises);
    
    // Save only the modified configs
    await Promise.all(savePromises);
    
    // Display appropriate message based on what was actually saved
    if (backendChanged) {
      message.warning('Backend configuration saved. The application will now restart to apply changes.');
    } else if (frontendChanged) {
      message.success('Frontend settings saved successfully!');
    }
  } catch (e) {
    console.error('Error saving settings:', e);
    message.error('Failed to save settings. Check console for details.');
  }
}

function addMapping() {
  editableButtonMappings.value.push({
    button_id: `custom_${Date.now()}`,
    button_label: '新按钮',
    script_key: 'new_script',
    script_type: 'bash',
    script_command: 'echo "Hello World"',
    description: '新添加的脚本映射',
    enabled: true
  });
}

function removeMapping(index) {
  editableButtonMappings.value.splice(index, 1);
}

async function saveButtonMappings() {
  try {
    // Update the config.toml file with new button mappings
    await configStore.saveButtonMappings(editableButtonMappings.value);
    message.success('Button mappings saved successfully!');
  } catch (e) {
    message.error('Failed to save button mappings.');
  }
}

function resetButtonMappings() {
  // Reset to original values from store
  editableButtonMappings.value = JSON.parse(JSON.stringify(configStore.buttonMappings || []));
  message.info('Button mappings reset to original values.');
}
</script>

<style scoped>
.settings-container {
  padding: 16px;
}

/* Make form items more compact - use higher specificity */
.settings-container .n-form-item {
  margin-bottom: 2px !important;
}

.settings-container .n-form-item .n-form-item-blank {
  min-height: auto !important;
  padding-top: 0 !important;
  padding-bottom: 0 !important;
}

/* Make input boxes more compact */
.settings-container .n-input.n-input--small {
  --n-height: 24px !important;
  --n-padding-left: 6px !important;
  --n-padding-right: 6px !important;
}

/* Target the specific Template Paths section */
.settings-container .n-card:first-child .n-form-item {
  margin-bottom: 2px !important;
}
</style>