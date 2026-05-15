# QuantNight Tauri 后端文档 (面向开发者)

本文档详细介绍了 `quantnight-frontend` 项目中 Tauri 后端（Rust 部分）的架构、功能模块及开发细节。

## 核心架构

后端采用 Rust 语言编写，基于 Tauri 框架，负责处理核心业务逻辑、数据库交互、外部脚本调用和与前端的通信。主要架构设计如下：

- **应用入口 (`main.rs` & `lib.rs`)**: 负责初始化 Tauri Builder，设置状态管理（State Management），注册所有暴露给前端的 Commands，并启动异步后台任务（如 Watcher）。
- **命令中心 (Commands Layer)**: 定义了所有 `#[tauri::command]` 接口，作为前端请求的第一个入口点。
- **数据访问层 (Data Layer)**: 基于 `mongodb` crate 实现，封装了复杂的聚合查询、分页和多库关联逻辑。

## 功能模块详解

| 模块 (Module) | 文件名 (File) | 核心职责 (Core Responsibilities) |
| :--- | :--- | :--- |
| **应用入口** | `main.rs` / `lib.rs` | 应用启动点, 注册命令, 初始化数据库连接池, 注入全局状态。 |
| **任务业务逻辑** | `commands.rs` | 处理任务的生命周期、文件 I/O、外部 Python/Bash 脚本调用。 |
| **高性能查询** | `datas.rs` | 负责 `alpha_db` 和 `data_db` 的复杂查询，支持聚合计算和动态排序。 |
| **指标计算** | `metrics.rs` | 处理 PNL 序列计算、统计分析等数学密集型任务。 |
| **配置管理** | `config.rs` | 加载并解析 `config.toml`，提供环境变量和全局路径的标准化访问。 |
| **前端特定配置** | `frontend_config.rs` | 维护前端界面所需的动态配置项，如集合列表、显示映射等。 |
| **数据库连接** | `mongo_manager.rs` | 封装 MongoDB 连接逻辑，提供单例模式的 `MongoClients`。 |
| **实时监听** | `watcher.rs` | 利用 MongoDB Change Streams 实时捕捉状态变更，通过 Event 推送到前端。 |

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
