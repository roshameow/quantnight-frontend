<template>
  <n-modal v-model:show="show" title="启动任务配置" preset="dialog">
    <n-form :model="form" label-placement="top">
      <n-switch :value="editConfig" @update:value="setConfigMode">
        <template #checked>编辑并覆盖本次配置</template>
        <template #unchecked>沿用核心保存配置</template>
      </n-switch>
      <n-p v-if="!editConfig">未提交配置覆盖；启动时由原核心按当前保存配置及默认规则选择并校验（等价 CLI 不传 --config）。此处不展示推断的有效 JSON。</n-p>
      <n-form-item v-else label="本次将提交的完整 JSON（显式覆盖）">
        <n-input type="textarea" v-model:value="form.config" autosize placeholder="请输入完整 JSON 对象；不会合并历史配置或自动回退默认值" />
      </n-form-item>
      <n-p v-if="editConfig && isPriority">{{ PRIORITY_CONFIG_HINT }}</n-p>
    </n-form>
    <template #action>
      <n-button @click="onCancel">取消</n-button>
      <n-button type="primary" @click="onConfirm">启动</n-button>
    </template>
  </n-modal>
</template>

<script setup>
import { ref, watch } from "vue";
import { useMessage } from "naive-ui";
import { parseTaskConfig, validatePriorityConfig, PRIORITY_CONFIG_HINT } from "@/utils/taskConfig";

const props = defineProps({
  show: Boolean,
  task: Object,
  isSuper: { type: Boolean, default: false },
  isPriority: { type: Boolean, default: false },
});
const emits = defineEmits(["update:show", "start-task", "start-super-task", "start-priority-task"]);
const message = useMessage();
const show = ref(props.show);
const form = ref({ config: "" });
const editConfig = ref(false);

// Mode is an explicit user choice, never inferred by comparing JSON to defaults.
const setConfigMode = (enabled) => {
  editConfig.value = enabled === true;
  if (!editConfig.value) form.value.config = "";
};
watch(() => [props.show, props.task, props.task?._id?.$oid ?? props.task?._id,
  props.task?.name, props.task?.isRemote, props.task?.auth_profile, props.task?.launchAttemptId,
  props.isSuper, props.isPriority], () => {
  show.value = props.show;
  if (props.show) {
    editConfig.value = false;
    form.value.config = "";
  }
}, { immediate: true });
watch(show, value => emits("update:show", value));
const onCancel = () => { setConfigMode(false); show.value = false; };
const onConfirm = () => {
  try {
    if (!props.task?.name) { message.error("任务信息缺失"); return; }
    const config = editConfig.value
      ? (props.isPriority ? validatePriorityConfig(form.value.config) : parseTaskConfig(form.value.config))
      : undefined;
    emits(props.isPriority ? "start-priority-task" : props.isSuper ? "start-super-task" : "start-task", {
      taskName: props.task.name,
      isRemote: props.task.isRemote,
      ...(editConfig.value ? { config } : {}),
    });
    show.value = false;
  } catch (error) {
    message.error("JSON 配置无效：" + error.message);
  }
};
</script>
