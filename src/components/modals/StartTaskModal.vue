<template>
  <n-modal v-model:show="show" title="启动任务配置" preset="dialog">
    <n-form :model="form" label-placement="top">
                  <n-form-item label="配置 JSON">
                    <n-input
                      type="textarea"
                      v-model:value="form.config"
                      autosize
                    />
                  </n-form-item>    </n-form>
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
  isPriority: {
    type: Boolean,
    default: false,
  },
});

const emits = defineEmits([
  "update:show",
  "start-task",
  "start-super-task",
  "start-priority-task",
]);

const message = useMessage();
const show = ref(props.show);
watch(
  () => props.show,
  (val) => (show.value = val)
);
watch(show, (val) => emits("update:show", val));

const form = ref({ config: "" });

// ✅ 普通任务 defaultJson
const adaptiveDefaultConfig = {
  enabled: false, // 改为 true 启用自适应调度（惰性打分 + LightGBM）
  train_interval: 500, // 攒够几条新结果触发重训
  sample_size: 50000, // 训练集抽样上限
  min_train: 100, // 最少训练样本数
  cand_batch: 10000, // 每轮拉取候选条数
  explore_ratio: 0.2, // 保底纯随机比例（探索）
  sat_power: 0.5, // 饱和惩罚强度（探索模式，0=关）
  max_same_df: 5, // 同一 datafield 同时在测上限
  good_th: [1.25, 0.8], // good 标记阈值 (sharpe, fitness)
  grade_bounds: [0.5, 1.25, 2.0], // grade 分档边界
};

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
  adaptive: adaptiveDefaultConfig,
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

// ✅ Priority 任务 defaultJson
const priorityDefaultJson = {
  interval: 600,
  priority: 1,
  filter_params: {
    sharpe_th: 1.2,
    fitness_th: 0.8,
    usage: "check",
  },
};

function generateDefaultConfig(task, isSuper, isPriority) {
  const template = isSuper
    ? superDefaultJson
    : isPriority
    ? priorityDefaultJson
    : normalDefaultJson;
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
      const config = generateDefaultConfig(props.task, props.isSuper, props.isPriority);
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

    if (props.isPriority) {
      emits("start-priority-task", {
        taskName: props.task.name,
        config: parsed,
        isRemote: props.task.isRemote,
      });
    } else if (props.isSuper) {
      emits("start-super-task", {
        taskName: props.task.name,
        config: parsed,
        isRemote: props.task.isRemote,
      });
    } else {
      emits("start-task", {
        taskName: props.task.name,
        config: parsed,
        isRemote: props.task.isRemote,
      });
    }

    show.value = false;
  } catch (e) {
    message.error("JSON 格式错误: " + e.message);
  }
};
</script>