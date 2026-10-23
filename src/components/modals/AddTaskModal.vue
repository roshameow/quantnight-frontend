<template>
  <n-modal
    v-model:show="showModal"
    preset="dialog"
    title="添加回测任务"
    :style="{ width: '700px' }"
  >
    <n-form label-placement="top">
      <n-form-item label="任务名称">
        <n-input
          v-model:value="form.taskName"
          placeholder="请输入任务名称"
         
        />
      </n-form-item>

      <n-form-item label="Auth Profile">
        <n-input
          v-model:value="form.authProfile"
          placeholder="请输入 Auth Profile (默认 user1)"
        />
      </n-form-item>

      <n-form-item label="添加模板路径（可选）">
        <n-input
          v-model:value="form.templatePath"
          placeholder="模版路径"
         
        />
      </n-form-item>

      <n-form-item label="模板名称（裸名称，支持旧目录模板）">
        <n-input v-model:value="form.templateFilename" placeholder="例如 my_template" />
        <n-button secondary @click="loadLatestTemplate">发现最新模板</n-button>
      </n-form-item>
      <n-text depth="3">选择文件会提取模板名称，关联后端已发布的同名模板；支持手动填写旧模板名。</n-text>
      <n-form-item>
        <template #label>
          <n-space align="center" style="justify-content: space-between; width: 100%">
            <span>模版:</span>
            <n-tag
              v-if="form.templateFilename"
              size="small"
              type="info"
              style="margin-left: auto"
            >
              <n-text depth="3" style="margin-left: auto">
                {{ form.templateFilename }}
              </n-text>
            </n-tag>
          </n-space>
        </template>

        <n-upload
          :show-file-list="true"
          :max="1"
          accept=".py,.json"
          @change="handleTemplateUpload"
        >
          <n-button secondary type="primary"> 选择 .py / .research.json 模板文件 </n-button>
        </n-upload>
      </n-form-item>
    </n-form>

    <template #action>
      <n-button @click="handleCancel">取消</n-button>
      <n-button type="primary" @click="handleSubmit">创建任务</n-button>
    </template>
  </n-modal>
</template>

<script setup>
import { ref, watch } from "vue";
import { useMessage } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import { useConfigStore } from "@/stores/configStore";
import { templateName } from "@/utils/templatePaths";

const configStore = useConfigStore();
let previousDefaultPath = configStore.paths.templatePath || "";


// ✅ 预填表单数据
const form = ref({
  taskName: "",
  authProfile: "user1",
  templatePath: previousDefaultPath,
  templateFilename: "",
  templateCode: "",
  isRemote: false, // ✅ 本地（false）/远程（true）标记
});

// props
const props = defineProps({ show: Boolean });
// emit
const emit = defineEmits(["update:show", "add-task"]);
// 局部状态绑定
const showModal = ref(props.show);

// watch 同步 props => state
watch(
  () => props.show,
  (v) => (showModal.value = v)
);
// watch 同步 state => emit 更新父组件
watch(showModal, (v) => emit("update:show", v));

const message = useMessage();



let latestRequestId = 0;
async function loadLatestTemplate() {
  const requestId = ++latestRequestId;
  const requestedPath = form.value.templatePath;
  const previousName = form.value.templateFilename;
  try {
    const latestTemplate = await invoke("read_latest_py_file", {
      folderPath: requestedPath,
      taskType: "regular",
    });
    if (requestId !== latestRequestId || form.value.templatePath !== requestedPath
        || form.value.templateFilename !== previousName) return;
    form.value.templateFilename = templateName(latestTemplate);
    form.value.templateCode = "";
  } catch (err) {
    console.error("读取模板失败:", err);
    message.warning(`自动选择模板失败：${err}。可手动填写旧模板名。`);
  }
}

watch(
  () => props.show,
  async (v) => {
    if (!v) return;
    await configStore.ready;
    const currentDefault = configStore.paths.templatePath || "";
    // Refresh a default, never overwrite an explicitly edited modal path/name.
    if (!form.value.templatePath || form.value.templatePath === previousDefaultPath) {
      form.value.templatePath = currentDefault;
    }
    previousDefaultPath = currentDefault;
    if (!form.value.templateFilename) await loadLatestTemplate();
  }
);

const handleTemplateUpload = ({ file }) => {
  if (file.status === "removed" || !file.file) return;
  let name;
  try { name = templateName(file.name); }
  catch (err) { message.error(err.message); return; }
  const reader = new FileReader();
  reader.onload = () => {
    form.value.templateFilename = name;
    // A descriptor is data consumed by the catalog, not Python source.
    form.value.templateCode = /\.py$/i.test(file.name) ? reader.result : "";
  };
  reader.readAsText(file.file);
};

const handleSubmit = () => {
  try { form.value.templateFilename = templateName(form.value.templateFilename); }
  catch (err) { message.error(err.message); return; }
  if (!form.value.taskName || !form.value.templateFilename) {
    message.error("请完整填写任务信息");
    return;
  }

  const newTask = {
    name: form.value.taskName,
    template: form.value.templateCode,
    templatefile: form.value.templateFilename || '',
    auth_profile: form.value.authProfile || "user1",
    status: "pending",
    createdAt: new Date(),
  };

  emit("add-task", newTask);
  showModal.value = false;
};


const handleCancel = () => {
  showModal.value = false;
};
</script>
