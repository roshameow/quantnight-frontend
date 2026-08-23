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
前端通过调用 `get_tasks` 初始化列表后，会监听由 `src-tauri/src/watcher.rs` 推送的实时事件（Tauri `listen("task-progress-update")`）。`taskStore.js` 维护一个 `progressMap`，以 `taskName` 为键实时更新渲染。

进度事件的底层依赖（详见下文「数据库连接与远程访问基础设施」）：
- 两端 mongod 都必须是**副本集**（本机与 mac-mini 均为单节点 `rs0`）——change stream 是 watcher 的数据源，非副本集时进度只剩启动快照，不实时更新。
- `isRemote=true` 的任务，watcher 聚合查询走 remote client（经 SSH 隧道），`false` 走本地 27017。
- 隧道服务 `com.quantnight.mongo-tunnel` 断线时，远程任务进度会冻结（属预期，KeepAlive 自动重连）。

### 2. 复杂表格渲染 (`useAlphaTableColumns.js`)
为了保持 View 组件的整洁，所有的 Alpha 列表列定义均抽离到 Composables 中。
- **渲染技术**：利用 `h` 函数动态生成 Naive UI 组件。
- **动态图表**：PNL 曲线采用 `vue-echarts`，在行展开时按需加载数据并渲染。

### 3. 数据集状态保持
为了优化用户体验，`DatasetPanel.vue` 不再使用本地 `ref` 存储搜索状态。所有输入框、分页和视图模式均绑定到 `datasetStore`。这确保了用户在查看 Datafield 后返回列表，或者切换到其他页面再回来时，搜索上下文完全保持。

## 数据库连接与远程访问基础设施

> 详细文档：`/Users/wenliu/Code/python/quantnight/docs/mongodb_infra_and_tunnel.md`（本仓库只列要点）。

### 拓扑与连接 URI

- **本机 mongod**：`/usr/local/mongodb/bin/mongod --config /usr/local/mongodb/mongod.conf`（dbPath=`/Users/wenliu/alphadb`，单节点副本集 `rs0`，127.0.0.1:27017）。由 LaunchAgent `com.quantnight.mongod` 托管（开机自启 + KeepAlive）。
- **远程执行机 mac-mini**：mongod 只绑定自身 `127.0.0.1:27017`，**无法直连**（旧 `192.168.1.100:27017` 已废弃）。唯一入口是 SSH 隧道 → 本机 `127.0.0.1:27018`（LaunchAgent `com.quantnight.mongo-tunnel`，走 Tailscale，内外网均通）。
- **连接配置**（Tauri `State<AppConfig>` 启动时读取）：
  - `src-tauri/config.toml`（dev）/ `src-tauri/resources/config.toml`（release 模板）
  - `local_uri = mongodb://127.0.0.1:27017/?directConnection=true`
  - `remote_uri = mongodb://127.0.0.1:27018/?directConnection=true`
  - `directConnection=true` 必须保留：单节点副本集的 hello 会广播自身 `127.0.0.1:27017`，不带此参数驱动会脱离隧道/连错库。
- **后端配套**：`credentials.yaml` 的 `mongodb.remote_uri` 同样指向隧道（sync_mission_list 推数据、删远程数据依赖）。

### 依赖的本机服务（launchctl）

| Label | 作用 | 检查 | 重启 |
|---|---|---|---|
| `com.quantnight.mongod` | 本机 mongod（rs0） | `nc -z 127.0.0.1 27017` | `launchctl kickstart -k gui/$(id -u)/com.quantnight.mongod` |
| `com.quantnight.mongo-tunnel` | SSH 隧道 → mac-mini（27018） | `nc -z 127.0.0.1 27018` | `launchctl kickstart -k gui/$(id -u)/com.quantnight.mongo-tunnel` |

### 故障排查速查

| 现象 | 检查点 | 处理 |
|---|---|---|
| 进度全 0 / 不动 | 本机 mongod 是否副本集 | `mongosh --eval 'db.getSiblingDB("admin").runCommand({hello:1}).setName'` 应输出 `rs0`，否则重启 `com.quantnight.mongod` |
| 远程任务进度冻结 | 隧道是否通 | `nc -z 127.0.0.1 27018`；mac-mini 开机且 Tailscale 保活；重启 `com.quantnight.mongo-tunnel` |
| 改了 config.toml 不生效 | AppConfig 是启动时快照 | 重启前端 app |

## UI 规范与样式
- **CSS 架构**：全局样式定义在 `src/assets/table-styles.css` 中，专门针对大数据量展示进行了性能优化（如虚拟列表单元格高度微调）。
- **组件交互**：遵循“数据流向下（Props），事件流向上（Emits）”的原则。
