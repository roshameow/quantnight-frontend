# 前端架构

本项目前端采用 Vue 3 (Composition API) 构建，并遵循“容器组件”与“展示组件”分离的设计模式，以提高代码的可维护性和复用性。

## 目录结构

-   **`src/main.js`**: 应用入口文件，负责初始化 Vue 应用、路由和 Pinia 状态管理。
-   **`src/router/index.js`**: 定义应用的页面路由。
-   **`src/stores/`**: 存放 Pinia store 模块。
    -   `taskStore.js`: 全局管理任务列表、任务进度等状态，实现跨组件共享。
-   **`src/views/`**: 存放页面级组件，作为“容器”，负责业务逻辑、数据获取和状态管理。
    -   `TaskManager.vue`: 任务管理页面的主容器。
    -   `DataPage.vue`: Alpha 数据分析页面的主容器。
-   **`src/components/`**: 存放可复用的“展示组件”，它们接收 props 并通过 emits 与父组件通信。
    -   `modals/`: 存放所有弹窗组件，如 `AddTaskModal`、`StartTaskModal` 等。
    -   `TaskCards/`: 存放与任务卡片相关的组件，如 `PriorityTaskCard`、`RegularTaskCard`。
    -   `data/`: 存放与数据展示相关的组件，如 `DataFilters`。
-   **`src/composables/`**: 存放可复用的 Vue Composition API 函数（“组合式函数”）。
    -   `useAlphaTableColumns.js`: 抽离了 `DataPage` 中复杂的表格列定义逻辑。

## 核心页面与组件拆分逻辑

### 1. 任务管理 (`TaskManager.vue`)

-   **`TaskManager.vue` (容器)**:
    -   通过 `invoke` 与 Tauri 后端通信，处理所有任务相关的增删改查操作。
    -   管理所有任务弹窗的显示/隐藏状态。
    -   监听子组件（如卡片、工具栏）发出的事件并执行相应逻辑。
-   **`TaskManagerToolbar.vue` (展示)**:
    -   显示顶部的“添加任务”按钮组。
    -   点击时，通过 `emits` 通知父组件打开相应的弹窗。
-   **`PriorityTaskCard.vue` / `RegularTaskCard.vue` (展示)**:
    -   接收 `task` 和 `progress` 对象作为 props 来渲染卡片UI。
    -   卡片上的所有操作按钮（如启动、暂停）都通过 `emits` 将事件和任务信息传递给父容器处理。

### 2. 数据分析 (`DataPage.vue`)

-   **`DataPage.vue` (容器)**:
    -   管理筛选条件、分页和排序的状态。
    -   调用 Tauri 后端获取 Alpha 数据。
    -   使用 `useAlphaTableColumns` 组合式函数来获取表格的列定义。
-   **`DataFilters.vue` (展示)**:
    -   包含所有的筛选输入框和选择器。
    -   使用 `v-model` 与父组件的 `filters` 对象双向绑定，实现状态同步。
-   **`useAlphaTableColumns.js` (逻辑复用)**:
    -   一个独立的函数，返回一个响应式的 `columns` 数组。
    -   封装了所有复杂的列渲染逻辑，包括自定义单元格、Popover 弹窗和内嵌的 ECharts PNL 图表，使 `DataPage.vue` 的代码更加简洁。