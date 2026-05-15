# 前端开发文档

本项目前端采用 Vue 3 (Composition API) 构建，基于 Vite 开发环境和 Naive UI 组件库。采用“容器组件”与“展示组件”分离的模式。

## 目录结构与模块职责

### 1. 核心状态管理 (Pinia Stores)

| Store | 文件名 | 职责 (Responsibilities) |
| :--- | :--- | :--- |
| **任务状态** | `taskStore.js` | 维护实时任务列表、进度映射及任务操作逻辑。 |
| **数据集状态** | `datasetStore.js` | 实现数据集面板的状态持久化（搜索条件、视图模式、分页）。 |
| **分析状态** | `analysisStore.js` | 管理 Alpha 列表的筛选状态及选中的 Alpha 详情。 |
| **配置状态** | `configStore.js` | 处理应用级全局配置、数据库切换及显示映射。 |
| **查询历史** | `queryStore.js` | 自动记录并持久化用户的高频搜索查询语句。 |

### 2. 页面容器 (Views)

| 页面 | 文件名 | 描述 |
| :--- | :--- | :--- |
| **任务管理** | `TaskManager.vue` | 核心调度界面，处理 WebSocket 事件及任务生命周期管理。 |
| **数据详情** | `DataPage.vue` | 基础 Alpha 列表页，支持高维度的组合过滤。 |
| **提交统计** | `SubmissionStatsPanel.vue` | 基于聚合数据的可视化统计，支持下钻到具体 Alpha 列表。 |
| **数据集管理** | `DatasetPanel.vue` | 两级数据结构管理，支持从 Dataset 下钻到 Datafield。 |
| **Alpha 分析** | `AnalysisPage.vue` | 包含图表对比、PNL 趋势分析的综合视图。 |

## 开发核心逻辑说明

### 1. 实时进度推送机制
前端通过调用 `get_tasks` 初始化列表后，会监听由 `src-tauri/src/watcher.rs` 推送的实时事件。`taskStore.js` 维护一个 `progressMap`，以 `taskName` 为键实时更新渲染。

### 2. 复杂表格渲染 (`useAlphaTableColumns.js`)
为了保持 View 组件的整洁，所有的 Alpha 列表列定义均抽离到 Composables 中。
- **渲染技术**：利用 `h` 函数动态生成 Naive UI 组件。
- **动态图表**：PNL 曲线采用 `vue-echarts`，在行展开时按需加载数据并渲染。

### 3. 数据集状态保持
为了优化用户体验，`DatasetPanel.vue` 不再使用本地 `ref` 存储搜索状态。所有输入框、分页和视图模式均绑定到 `datasetStore`。这确保了用户在查看 Datafield 后返回列表，或者切换到其他页面再回来时，搜索上下文完全保持。

## UI 规范与样式
- **CSS 架构**：全局样式定义在 `src/assets/table-styles.css` 中，专门针对大数据量展示进行了性能优化（如虚拟列表单元格高度微调）。
- **组件交互**：遵循“数据流向下（Props），事件流向上（Emits）”的原则。
