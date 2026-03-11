<template>
  <n-modal
    v-model:show="showModal"
    preset="dialog"
    title="添加Super Alpha回测任务"
    :style="{ width: '700px' }"
  >
    <n-form label-placement="top">
      <n-form-item label="任务名称">
        <n-input
          v-model:value="form.taskName"
          placeholder="请输入任务名称"
         
        />
      </n-form-item>

      <n-form-item label="添加模板路径（可选）">
        <n-input
          v-model:value="form.templatePath"
          placeholder="模版路径"
         
        />
      </n-form-item>

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
                {{ form.templateFilename }}.py
              </n-text>
            </n-tag>
          </n-space>
        </template>

        <n-upload
          :show-file-list="true"
          :max="1"
          accept=".py"
          @change="handleTemplateUpload"
        >
          <n-button secondary type="primary"> 上传 .py 模板文件 </n-button>
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
import { ref, watch, onMounted } from "vue";
import { useMessage } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import { AppConfig } from "@/config";



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



// ✅ 预填表单数据
const form = ref({
  taskName: "",
  templatePath: AppConfig.paths.superTemplatePath || '',
  templateFilename: "",
  templateCode: "",
  isRemote: false, // ✅ 本地（false）/远程（true）标记
  taskType: "super",
});

watch(
  () => props.show,
  async (v) => {
    showModal.value = v;
    if (v) {
      try {
        const latestTemplate = await invoke("read_latest_py_file", {
          folderPath: form.value.templatePath,
        });
        form.value.templateFilename = latestTemplate.replace(/\.py$/i, "");
        console.log("添加任务弹窗打开时，最新模板文件:", form.value.templateFilename);
      } catch (err) {
        console.error("读取模板失败:", err);
        message.warning("无法自动读取模板文件，请手动上传");
      }
    }
  }
);



const handleTemplateUpload = ({ file }) => {
  const reader = new FileReader();
  reader.onload = () => {
    form.value.templateCode = reader.result;
    form.value.templateFilename = file.name.replace(/\.py$/i, ""); // 存储文件名
    console.log("templateFilename:：", form.value.templateFilename);
  };
  reader.readAsText(file.file);
};

const handleSubmit = () => {
  if (!form.value.taskName || !form.value.templateFilename) {
    message.error("请完整填写任务信息");
    return;
  }

  try {
    const newTask = {
      // id: Date.now().toString(),
      name: form.value.taskName,
      template: form.value.templateCode,
      templatefile: form.value.templateFilename,
      status: "pending",
      createdAt: new Date(),
      isSuper: true, // ✅ 加上这一行
    };

    emit("add-super-task", newTask);
    message.success("任务创建成功");
    showModal.value = false;
  } catch (e) {
    console.error(e);
    message.error("配置 JSON 格式错误！");
  }
};

const handleCancel = () => {
  showModal.value = false;
};
</script>
