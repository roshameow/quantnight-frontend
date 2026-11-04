# QuantNight Tauri 后端文档 (面向开发者)

本文档详细介绍了 `quantnight-frontend` 项目中 Tauri 后端（Rust 部分）的架构、功能模块及开发细节。

## 核心架构

Tauri Rust层负责WebView命令、UI事件映射及非任务数据能力。明确runtime登记后，**任务域**统一委托task-core公共Unix IPC client与独立headless taskctl host；GUI与CLI平级，不各写codec/身份/错误/dispatcher。schema1未启用runtime保留原embedded模式，已登记错误不fallback。本文描述源码合同，不是安装/loaded证明；构建、登记与drain采用只见[taskctl手册](../crates/taskctl/README.md#runtime-enrollment-and-safe-adoption)。主要职责如下：

- **应用入口 (`main.rs` & `lib.rs`)**：注册原Commands/配置/非任务数据池和一次创建的任务endpoint。enrolled任务endpoint只持公共client，任务watch投影/查询由host共享任务slots；关闭GUI只断客户端/订阅，不停止host。
- **命令中心 (Commands Layer)**：保留原 `#[tauri::command]` WebView入口；任务统一构造 `TaskRequest` 并调公共 `TaskEndpoint.execute(TaskRequest, observer)` / `RuntimeClient`，不重写dispatcher、codec或持久transport receipt服务。binding/runtime schema与OS image/process identity只复用 `crates/runtime_registration.rs` / `crates/native_identity.rs`；详见上方唯一手册源码索引。
- **数据访问层 (Data Layer)**：非任务 `datas.rs/metrics.rs` 的Alpha/data/PNL/correlation原功能、GUI独立池与namespace设置保留。任务列表/remote-only缓存、diagnostics、进度投影全部属于公共host，不能偷用GUI数据池。原GUI本来已复用Arc连接，不是每按钮重连。

## 功能模块详解

| 模块 (Module) | 文件名 (File) | 核心职责 (Core Responsibilities) |
| :--- | :--- | :--- |
| **应用入口** | `main.rs` / `lib.rs` | 注册命令/任务endpoint与非任务数据状态；GUI loaded与host loaded分别核验。 |
| **任务适配** | `commands.rs` | 任务参数/回执/阶段UI映射；公共client→host→原core→原脚本owner，不直接执行第二套任务状态机。 |
| **高性能查询** | `datas.rs` | 负责 `alpha_db` 和 `data_db` 的复杂查询，支持聚合计算和动态排序。 |
| **指标计算** | `metrics.rs` | 处理 PNL 序列计算、统计分析等数学密集型任务。 |
| **配置管理** | `config.rs` | 原AppConfig loader保留；GUI任务host明确app_config selector→resolve_gui_binding→MongoConfig，核原check_task_status脚本/任务namespace，非任务/UI设置保持。纯CLI backend_root不补造GUI脚本；不经IPC传整份配置。 |
| **前端特定配置** | `frontend_config.rs` | 维护前端界面所需的动态配置项，如集合列表、显示映射等。 |
| **数据库连接** | `mongo_manager.rs` | 原core Mongo工厂兼容引用；非任务GUI池保留，任务池唯一归host，端点权威仍是原backend MongoConfig。 |
| **实时监听适配** | `watcher.rs` | 订阅公共host的原任务progress事件并转Tauri emit；Change Streams/查询投影在core任务owner，多订阅共用而非每窗口开池。 |

## 核心 API 功能开发说明

### 1. Alpha 结果处理 (`get_alpha_results`)
- **逻辑**：支持基于 `is` 子文档指标的数值范围过滤，以及对 `checks` 数组的逻辑过滤。
- **扩展**：在数据返回给前端前，会在 Rust 层对 `checks` 进行预处理，将结果中的 `value` 与 `limit` 拼接成易读字符串。

### 2. 数据集与字段引擎
- **`get_datasets`**: 实现了数据集级别的反向查找功能（通过 `field_id` 查找所属 Dataset）。
- **`get_datafields`**: 
    - 深度整合了 MongoDB 的 `$filter` 和 `$reduce` 聚合操作。
    - **排序机制**：排序字段 `sortValue` 会根据前端传入的 `Region/Delay` 动态生成。例如，在选择特定区域时，排序将仅基于该区域的 `alphaCount`，而不是全区域总和。

### 3. 统计聚合 (`get_submission_stats`)
- 采用 `$group` 操作符实现按 `(month, region, delay)` 的多维聚合。

## 数据库架构与字段

后端交互的数据库及其核心职责：

-   **`simulation_mission` (任务元数据):** 存储任务定义 (`tasks` 集合)，包含 `status`, `taskType`, `mission_config` 等。
-   **`simulation_db` (执行过程库):** 存储每个具体任务名对应的实时产出数据。
-   **`alpha_db` (策略结果库):** 包含 `alpha_results` 及其相关 PNL 序列。
-   **`dataset_db` / `data_db` (元数据统计库):** 包含 `datasets_all` 和 `datafields_all` 集合。

### 关键数据模型结构 (Developer Reference)

- **`AlphaResult`**: 
  - 嵌套结构：`is:指標文档`, `settings:环境配置`, `checks:检查结果数组`。
- **`Datafield`**:
  - `dateCoverage`: 字段的时间覆盖比例。
  - `data`: 包含 `region`, `delay`, `universe`, `coverage`, `alphaCount` 的数组。
