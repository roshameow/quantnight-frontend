<template>
  <n-modal v-model:show="show" title="启动任务配置" preset="dialog">
    <n-form :model="form" label-placement="top">
      <n-form-item label="配置 JSON">
        <n-input type="textarea" v-model:value="form.config" autosize />
      </n-form-item>
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

const props = defineProps({
  show: Boolean,
  task: Object,
  isSuper: {
    type: Boolean,
    default: false,
  },
});

const emits = defineEmits(["update:show", "start-task", "start-super-task"]);

const message = useMessage();
const show = ref(props.show);
watch(() => props.show, (val) => (show.value = val));
watch(show, (val) => emits("update:show", val));

const form = ref({ config: "" });

// ✅ 普通任务 defaultJson
const normalDefaultJson = {
  max_concurrent: 8,
  max_multi_simulation_children: 10,
  tag: {
    name: "Unknown Task",
    tags: ["Unknown Task"],
    regular: {
      description:
        "Idea: single dataset alpha\nRationale for data used: lower than 3\nRationale for operators used: lower than 8",
    },
  },
};

// ✅ Super 任务 defaultJson
const superDefaultJson = {
  max_concurrent: 3,
  max_multi_simulation_children: 1,
  tag: {
    name: "Unknown Task",
    tags: ["Unknown Task"],
    selection: {
      description:
        "Idea: single dataset alpha\nRationale for data used: lower than 3\nRationale for operators used: lower than 8",
    },
    combo: {
      description:
        "Idea: single dataset alpha\nRationale for data used: lower than 3\nRationale for operators used: lower than 8",
    },
  },
};

function generateDefaultConfig(task, isSuper) {
  const template = isSuper ? superDefaultJson : normalDefaultJson;
  const cloned = JSON.parse(JSON.stringify(template)); // 深拷贝

  const taskName = task?.name || "Unknown Task";
  if (cloned.tag) {
    cloned.tag.name = taskName;
    cloned.tag.tags = [taskName];
  }

  return cloned;
}

watch(
  () => props.show,
  (val) => {
    show.value = val;
    console.log("watch:：props.isSuper", props.isSuper);


    // ✅ 每次打开弹窗都重新生成 config
    if (val && props.task) {
      const config = generateDefaultConfig(props.task, props.isSuper);
      form.value.config = JSON.stringify(config, null, 2);
    }
  },
  { immediate: true }
);




const onCancel = () => {
  show.value = false;
};

const onConfirm = () => {
  try {
    const parsed = JSON.parse(form.value.config);

    if (!props.task || !props.task.name) {
      message.error("任务信息缺失，无法启动");
      return;
    }

    emits(props.isSuper ? "start-super-task" : "start-task", {
      taskName: props.task.name,
      config: parsed,
      isRemote: props.task.isRemote,
    });

    show.value = false;
  } catch (e) {
    message.error("JSON 格式错误，请检查输入");
  }
};
</script>
