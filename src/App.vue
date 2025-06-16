<script>
import AppLayout from "./components/AppLayout.vue";
import { RouterLink, RouterView } from "vue-router";
import TaskManager from "./views/TaskManager.vue";
import { NConfigProvider, NDialogProvider, NMessageProvider } from "naive-ui";


export default {
  components: {
    AppLayout,
    TaskManager,
  },
};
</script>

<template>
  <n-config-provider>
    <n-dialog-provider>
      <n-message-provider>
        <AppLayout>
          <router-view />
        </AppLayout>
      </n-message-provider>
    </n-dialog-provider>
  </n-config-provider>
</template>
