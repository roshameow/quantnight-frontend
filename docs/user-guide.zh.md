## 首次使用与配置

在开始使用前，请确保您的环境已正确配置。

### 1. 环境准备

- **MongoDB 数据库**: 应用需要连接到一个正在运行的 MongoDB 实例。请确保实例中已创建了以下三个数据库：
  1.  `simulation_mission`
  2.  `simulation_db`
  3.  `alpha_db`
- **创建 `tasks` 集合**: 在 `simulation_mission` 数据库中，您**必须**手动创建一个名为 `tasks` 的空集合，否则应用将无法正常启动。

### 2. 配置管理

应用TOML选择 `env.working_dir` 后端根、UI库/集合和脚本设置；连接端点以该后端原MongoConfig（私有 `credentials.yaml`）为权威，不使用TOML旧URI字段。Settings不替代后端凭据/连接配置；不要上传凭据或回执。

**macOS明确登记runtime后**，GUI与CLI是同一headless任务host的平级客户端。用户级launchd socket在首次使用时启动host，不需要先开GUI、不经MCP。任务列表/remote-only缓存、诊断、操作与进度投影都由该host负责；Alpha/data/PNL/correlation等非任务功能仍保留GUI原独立数据池。

- 构建、登记和安全采用见[唯一构建/部署手册](../crates/taskctl/README.md#runtime-enrollment-and-safe-adoption)，业务CLI见[使用手册](cli-usage.md)。拖入.app或修改磁盘文件不等于host已登记/加载。
- 关闭GUI不停止host或回测runner，不取消已接受平台请求；重新打开GUI只重新接入客户端。操作断连/失响应不能自动重发，先核原任务/generation/launch/业务回执。
- UI设置仍沿原界面规则采用；GUI非任务数据配置按原应用重开加载。**任务host配置/共享源码变化不能仅靠重启GUI生效**：由原部署owner先drain已接操作，再明确换代并核host loaded identity，不能杀busy host。
- GUI任务诊断需要host明确选择原canonical AppConfig，保留check_task_status脚本合同；原loader仍使用MongoConfig连接权威。部署候选旗标为 `--runtime-app-config`（见唯一手册），不上传全AppConfig/URI或任意脚本；backend_root只保留纯CLI语义。已有schema2仅升exe无新cfg保留selector，换selector先旧binding安全停止再发布，不先改指针。
- schema1未启用runtime时保留原模式；第一版runtime登记仅支持macOS，Windows等原未启用路径不删除。本文描述源码合同，不声称现有安装包已经采用。

---

## 使用指南

### 任务管理器 (Task Manager)

这是应用的主界面，您可以在这里看到所有已创建的任务列表。

- **查看任务**: 列表会显示任务的名称、类型、状态等信息。
- **操作任务**: 每个任务卡片上都提供了 `启动` / `暂停` / `删除` 等常用操作按钮。
- **任务卡片进度**: 卡片显示 `✓ 成功 | ✗ 失败 | Σ 总计`（格式为 `普通数+插队数`）。进度由 watcher 实时推送，isRemote 任务显示的是**远程库**的状态。

### 创建新任务

点击界面右上角的 `+ Add Task` 按钮，可以选择创建三种不同类型的任务：

- **Add Task**: 创建一个常规的量化任务。
- **Add Super Task**: 创建一个超级任务，通常用于执行更复杂的组合或流程。
- **Add Priority Task**: 创建一个高优先级的任务。

在弹出的窗口中，您需要填写任务名称并选择一个合适的模板文件来生成任务。启动时可在"配置 JSON"中设置任务参数（如 `adaptive` 自适应调度配置）。

### 数据分析 (Data)

切换到 `Data` 页面，可以对任务产生的结果进行深入分析。

- **选择结果集**: 在顶部的下拉菜单中选择您想分析的数据集合（例如 `alpha_results`）。
- **使用筛选器**: 利用上方的多个筛选框（如 ID, Region, Turnover 等）来精确地过滤数据。
- **计算相关性**: 在筛选出您关心的数据后，点击 `计算 Correlation` 按钮，应用将对结果进行关联性分析。

### 应用设置 (Settings)

点击界面右上角的齿轮图标 `⚙️` 可以打开设置面板。在这里，您可以方便地修改所有应用配置：

- **前端配置**:
  - **Template Paths**: 设置用于创建不同类型任务的模板文件所在的文件夹路径。
  - **Data Filter Options**: 自定义"数据分析"页面中结果集下拉菜单的选项。

- **后端配置**:
  - **MongoDB 设置**: UI库/集合名仍按原设置；连接URI来自后端原MongoConfig，不以TOML中的旧URI覆盖。任务namespace必须匹配登记，非任务Alpha/data/PNL保留原设置。
  - **Python 设置**: 设置 Python 解释器路径和工作目录。
  - **脚本映射**: 管理按钮与后端脚本的映射关系，支持添加自定义脚本。

> **注意**：GUI非任务数据池与任务host分别加载配置；任务连接/共享源码变化按上方部署手册安全采用，重开GUI不能替代host换代。

---

## 应用安装

以下是桌面包安装，不等于runtime host登记或运行采用；macOS任务host需另按上方部署手册明确采用，不能据此推断launchd已安装。

### macOS

1.  **下载**: 前往项目的 Releases 页面下载最新的 `.dmg` 文件。
2.  **安装**: 打开 `.dmg` 文件，将 `QuantNight.app` 拖入 `Applications` 文件夹。
3.  **安全提示**: 首次打开时，如果 macOS 提示“应用已损坏”或“无法打开”，请在终端中执行以下命令，然后即可正常打开：
    ```bash
    sudo xattr -r -d com.apple.quarantine /Applications/QuantNight.app
    ```

### Windows

1.  **下载**: 前往项目的 Releases 页面下载最新的 `.msi` 安装包。
2.  **安装**: 双击 `.msi` 文件，按照向导完成安装。
3.  **安全提示**: 首次运行时，如果 Windows SmartScreen 弹出拦截提示，请点击 `更多信息` -> `仍要运行`。
