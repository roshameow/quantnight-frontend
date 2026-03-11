<template>
  <n-modal v-model:show="visible" title="添加任务流" preset="dialog">
    <div>
      <n-input spellcheck="false" v-model:value="flowName" placeholder="输入任务流名称" class="mb-3" spellcheck="false" />
      <n-checkbox-group v-model:value="selectedScripts">
        <n-space vertical>
          <n-checkbox
            v-for="script in scriptPool"
            :key="script.script"
            :value="script"
            :label="script.name + ' (' + script.script + ')'"
          />
        </n-space>
      </n-checkbox-group>
    </div>
    <template #action>
      <n-button @click="$emit('close')">取消</n-button>
      <n-button type="primary" @click="submitFlow">确定</n-button>
    </template>
  </n-modal>
</template>

<script setup>
import { ref } from 'vue'
import { NModal, NInput, NButton, NCheckboxGroup, NCheckbox, NSpace } from 'naive-ui'

defineProps({
  scriptPool: Array
})

const visible = ref(true)
const flowName = ref('')
const selectedScripts = ref([])

function submitFlow() {
  if (!flowName.value || selectedScripts.value.length === 0) return
  // 触发上层的 addNewFlow
  emit('submit', flowName.value, selectedScripts.value)
  emit('close')
}
const emit = defineEmits(['submit', 'close'])
</script>

<style scoped>
.mb-3 {
  margin-bottom: 1rem;
}
</style>
