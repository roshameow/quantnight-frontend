# QuantNight Tauri 后端文档

本文档详细介绍了 `quantnight-frontend` 项目中 Tauri 后端（Rust 部分）的架构和功能模块。

## 核心架构

后端采用 Rust 语言编写，基于 Tauri 框架，负责处理核心业务逻辑、数据库交互、外部脚本调用和与前端的通信。主要架构包括：

- **应用入口 (`main.rs`)**: 初始化 Tauri 应用，加载配置文件，建立数据库连接池，并注册所有暴露给前端的命令。
- **命令中心 (`commands.rs`)**: 定义了所有前端可以调用的接口（Tauri Commands），是前后端交互的核心。
- **配置与脚本执行器 (`config.rs`)**: 负责加载 `config.toml` 配置文件，并提供了标准化的函数来执行外部的 Python 和 Bash 脚本。
- **数据模型与查询 (`datas.rs`)**: 定义了 Alpha 数据的结构体，并提供了复杂的数据查询接口，专门用于从 `alpha_db` 获取数据。
- **数据库管理器 (`mongo_manager.rs`)**: 封装了与本地和远程 MongoDB 的连接逻辑，提供一个线程安全的数据库客户端实例。
- **实时数据监听器 (`watcher.rs`)**: 利用 MongoDB Change Streams 实时监控数据库中任务状态的变化，并通过 WebSocket 将更新主动推送给前端。

## 功能模块详解

| 模块 (Module) | 文件名 (File) | 核心职责 (Core Responsibilities) |
| :--- | :--- | :--- |
| **应用入口** | `main.rs` | 应用启动点, 加载配置, 初始化数据库连接池, 注册所有 Tauri 命令。 |
| **核心业务命令** | `commands.rs` | 定义所有前端可调用的接口，处理任务生命周期、执行控制和外部交互。 |
| **配置与脚本执行** | `config.rs` | 加载 `config.toml`，提供执行外部 Python/Bash 脚本的标准化函数。 |
| **Alpha 数据查询** | `datas.rs` | 专门处理对 `alpha_db` 的复杂查询，包括分页、排序和过滤。 |
| **数据库连接** | `mongo_manager.rs` | 封装与 MongoDB 的连接逻辑，提供线程安全的客户端实例。 |
| **实时进度监听** | `watcher.rs` | 监控数据库变化，通过 WebSocket 将任务进度实时推送给前端�� |

## 数据库结构

后端主要依赖以下三个核心 MongoDB 数据库，各自承担不同职责：

-   **`simulation_mission` (任务元数据库):** 负责存储所有任务的定义和元数据。它是任务的“指挥中心”，记录了有哪些任务、它们的类型、状态和配置。
-   **`simulation_db` (任务实时数据库):** 用于存储每个任务在执行过程中产生的实时、详细数据。数据库中的每个集合都与 `simulation_mission` 中的一个任务相对应。
-   **`alpha_db` (Alpha 分析结果库):** 专门用于存储 Alpha 策略的最终分析和回测结果，包括性能指标、PNL 时间序列等，供数据分析和展示使用。

### 详细字段总览

下表汇总了上述数据库中核心集合的字段结构。

| 数据库 (Database) | 集合 (Collection) | 字段 (Field) | 类型 (Type) | 描述 (Description) |
| :--- | :--- | :--- | :--- | :--- |
| **`simulation_mission`** | **`tasks`** | `_id` | `ObjectId` | 任务的唯一标识。 |
| | | `name` | `String` | 任务名称，**此名称将用作 `simulation_db` 中的集合名**。 |
| | | `template` | `String` | 任务模��的内容或标识。 |
| | | `templatefile` | `String` | 任务使用的模板文件名。 |
| | | `status` | `String` | 任务状态 (`waiting`, `ready`, `running`, `paused`, `deactive`)。 |
| | | `isRemote` | `Boolean` | 标记任务是否在远程服务器上执行。 |
| | | `taskType` | `String` | 任务类型 (`regular`, `super`, `priority`)。 |
| | | `mission_config` | `Document` | (可选) 任务启动时使用的具体配置参数。 |
| **`simulation_db`** | **`<task_name>`** | `status` | `String` | 子任务或数据点的状态，关键值为 `success`。 |
| | *(动态命名)* | `priority` | `Number` | 优先级，用于区分优先任务 (`> 0`)。 |
| | | `...` | `Any` | 其他字段由外部 Python/Bash 脚本决定和写入。 |
| **`alpha_db`** | **`alpha_results`** | `id` | `String` | Alpha 的唯一标识。 |
| | *(或其他动态集合)* | `region` | `String` | 区域 (e.g., "CN", "US")。 |
| | | `type` | `String` | Alpha 类型 ("REGULAR", "SUPER")。 |
| | | `code` | `String` | Alpha 的代码表达式。 |
| | | `dateCreated` | `String` | (ISO Date) 创建日期。 |
| | | `pnl_score` | `Number` | (可选) PNL 分数。 |
| | | `settings` | `Document` | 包含 `region`, `delay` 等设置的子文档。 |
| | | `is` | `Document` | 包含核心指标 (`sharpe`, `fitness`, `returns` 等) 的子文档。 |
| | | `pnl` | `Document` | 包含 PNL 时间序列 (`records`) 的子文档。 |