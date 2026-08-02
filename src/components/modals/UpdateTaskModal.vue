<template>
  <n-modal v-model:show="show" title="更新任务配置" preset="dialog">
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
      <n-button type="primary" @click="onConfirm">更新</n-button>
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

const emits = defineEmits(["update:show", "update-task", "update-priority-task"]);
const message = useMessage();
const show = ref(props.show);
watch(
  () => props.show,
  (val) => (show.value = val)
);
watch(show, (val) => emits("update:show", val));

const form = ref({ config: "" });

// ---------- Default Configs ----------
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

const priorityDefaultJson = {
  interval: 600,
  priority: 1,
  filter_params: {
    sharpe_th: 1.2,
    fitness_th: 0.8,
    usage: "check",
  },
};

// ---------- 生成默认配置 ----------
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

// ---------- 初始化表单 ----------
watch(
  () => props.show,
  (val) => {
    show.value = val;
    if (val && props.task) {
      const config = generateDefaultConfig(props.task, props.isSuper, props.isPriority);
      form.value.config = JSON.stringify(config, null, 2);
    }
  },
  { immediate: true }
);

const onCancel = () => (show.value = false);

const onConfirm = () => {
  try {
    const parsed = JSON.parse(form.value.config);
    if (!props.task || !props.task.name) {
      message.error("任务信息缺失，无法更新");
      return;
    }

    if (props.isPriority) {
      // Priority任务走这个
      emits("update-priority-task", {
        taskName: props.task.name,
        config: parsed,
        isRemote: props.task.isRemote,
      });
    } else {
      // 普通 / super任务走这个
      emits("update-task", {
        taskName: props.task.name,
        config: parsed,
        isRemote: props.task.isRemote,
      });
    }

    show.value = false;
  } catch (e) {
    message.error("JSON 格式错误，请检查输入");
  }
};
</script>