<template>
  <n-modal
    v-model:show="showModal"
    preset="dialog"
    title="添加 Priority 精炼任务"
    :style="{ width: '700px' }"
  >
    <n-form label-placement="top">
      <n-form-item label="任务名称（可自定义）">
        <n-input v-model:value="form.taskName" placeholder="pipeline__priority" />
      </n-form-item>
      <n-form-item label="Auth Profile">
        <n-input v-model:value="form.authProfile" placeholder="user1" />
      </n-form-item>
      <n-form-item label="Priority 运行主机（独立于 root 来源执行主机）">
        <n-switch v-model:value="form.isRemote">
          <template #checked>远程</template>
          <template #unchecked>本地</template>
        </n-switch>
      </n-form-item>
      <n-p>
        使用内置 pipeline 模板，任务名称可自定义。
        创建时注册任务并保存配置；通过启动按钮运行。默认持续接收所选Priority主机control注册表中同账号的活跃 regular 任务真实成功结果（来源按immutable source_remote独立执行），
        前端仅启动和监控，生成在所选运行主机执行，每个 root 独立3阶段（每阶段≤60、累计≤180）。
        可在 JSON 中设置 source_tasks 过滤来源；省略则动态选择活跃来源。
      </n-p>
      <n-form-item label="配置 JSON（高级：模板开关、参数与预算）">
        <n-input type="textarea" v-model:value="form.configText" :autosize="{ minRows: 10, maxRows: 24 }" />
      </n-form-item>
      <n-p>{{ PRIORITY_CONFIG_HINT }}</n-p>
    </n-form>
    <template #action>
      <n-button @click="showModal = false">取消</n-button>
      <n-button type="primary" @click="handleSubmit">创建任务</n-button>
    </template>
  </n-modal>
</template>

<script setup>
import { ref, watch } from "vue";
import { useMessage } from "naive-ui";
import { priorityDefaultConfig, priorityTaskPayload, PRIORITY_CONFIG_HINT } from "@/utils/taskConfig";

const props = defineProps({ show: Boolean });
const emit = defineEmits(["update:show", "add-priority-task"]);
const showModal = ref(props.show);
const form = ref({
  taskName: "pipeline__priority",
  authProfile: "user1",
  isRemote: true,
  configText: JSON.stringify(priorityDefaultConfig(), null, 2),
});
const message = useMessage();
watch(() => props.show, (value) => (showModal.value = value));
watch(showModal, (value) => emit("update:show", value));

const handleSubmit = () => {
  try {
    const task = priorityTaskPayload(form.value.taskName, form.value.authProfile, form.value.configText, form.value.isRemote);
    emit("add-priority-task", task);
    // Parent reports persistence success/failure after awaiting the command.
    showModal.value = false;
  } catch (error) {
    message.error("配置无效: " + error.message);
  }
};
</script>
