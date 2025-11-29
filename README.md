# QuantNight 量化任务管理器

QuantNight 是一款专为量化研究与模拟设计的桌面应用，旨在帮助您高效地管理、执行和分析各类量化任务。

<!-- 在此处可以放置一张应用的截图 -->
<!-- ![QuantNight App Screenshot](link_to_screenshot.png) -->

---

## 核心功能

- **统一的任务管理**：在一个清晰的界面中查看所有普通、优先和超级任务。
- **灵活的任务创建**：通过模板快速创建新任��，支持本地或远程执行。
- **便捷的任务操作**：一键启动、暂停、更新或删除任务。
- **数据可视化与分析**：内置数据浏览器，支持对结果集进行筛选、排序和关联性分析。
- **图形化应用配置**：通过设置界面，轻松管理前后端的所有关键配置，无需手动修改文件。

---

## 首次使用与配置

在开始使用前，请确保您的环境已正确配置。

### 1. 环境准备

- **MongoDB 数据库**: 应用需要连接到一个正在运行的 MongoDB 实例。请确保实例中已创建了以下三个数据库：
  1.  `simulation_mission`
  2.  `simulation_db`
  3.  `alpha_db`
- **创建 `tasks` 集合**: 在 `simulation_mission` 数据库中，您**必须**手动创建一个名为 `tasks` 的空集合，否则应用将无法正常启动。

```mermaid
graph TD
    A[用户通过 UI 创建任务] --> B(<b>simulation_mission</b><br/><i>存储任务配置和状态</i>);
    B --> C{后台任务执行};
    C --> D(<b>simulation_db</b><br/><i>存储原始模拟数据</i>);
    C --> E(<b>alpha_db</b><br/><i>存储核心 Alpha 结果</i>);
    E --> F[数据分析页面读取结果];
```

### 2. 配置文件

应用启动后，大部分配置都可以通过图形化界面完成。您只需确保后端的 `src-tauri/config.toml` 文件存在且包含基本的数据库连接信息。

**`config.toml` 基础示例:**
```toml
[mongodb]
# 本地 MongoDB 连接字符串
local_uri = "mongodb://localhost:27017"
# 远程 MongoDB 连接字符串 (若无，可指向本地)
remote_uri = "mongodb://localhost:27017"

[mongodb.databases]
mission = "simulation_mission"
simulation = "simulation_db"
alpha = "alpha_db"

# 其他后端脚本所需的配置...
[python]
# ...

[bash]
# ...
```
> **提示**: 首次启动应用后，您可以通过 `Settings` 按钮在界面中直接修改此文件的内容。

---

## 使用指南

### 任务管理器 (Task Manager)

这是应用的主界面，您可以在这里看到所有已创建的任务列表。

- **查看任务**: 列表会显示任务的名称、类型、状态等信息。
- **操作任务**: 每个任务卡片上都提供了 `启动` / `暂停` / `删除` 等常用操作按钮。

### 创建新任务

点击界面右上角的 `+ Add Task` 按钮，可以选择创建三种不同类型的任务：

- **Add Task**: 创建一个常规的量化任务。
- **Add Super Task**: 创建一个超级任务，通常用于执行更复杂的组合或流程。
- **Add Priority Task**: 创建一个高优先级的任务。

在弹出的窗口中，您需要填写任务名称并选择一个���适的模板文件来生成任务。

### 数据分析 (Data)

切换到 `Data` 页面，可以对任务产生的结果进行深入分析。

- **选择结果集**: 在顶部的下拉菜单中选择您想分析的数据集合（例如 `alpha_results`）。
- **使用筛选器**: 利用上方的多个筛选框（如 ID, Region, Turnover 等）来精确地过滤数据。
- **计算相关性**: 在筛选出您关心的数据后，点击 `计算 Correlation` 按钮，应用将对结果进行关联性分析。

### 应用设置 (Settings)

点击界面右上角的齿轮图标 `⚙️` 可以打开设置面板。在这里，您可以方便地修改所有应用配置：

- **前端配置**:
  - **Template Paths**: 设置用于创建不同类型任务的模板文件所在的文件夹路径。
  - **Data Filter Options**: 自定义“数据分析”页面中结果集下拉菜单的选项。
- **后端配置**:
  - **Backend Config (config.toml)**: 直接编辑后端 `config.toml` 文件的内容，例如修改数据库连接字符串或脚本路径。

---

## 应用安装

### macOS

1.  **下载**: 前往 [Releases 页面](https://github.com/你��仓库/releases) 下载最新的 `.dmg` 文件。
2.  **安装**: 打开 `.dmg` 文件，将 `QuantNight.app` 拖入 `Applications` 文件夹。
3.  **安全提示**: 首次打开时，如果 macOS 提示“应用已损坏”或“无法打开”，请在终端中执行以下命令，然后即可正常打开：
    ```bash
    sudo xattr -r -d com.apple.quarantine /Applications/QuantNight.app
    ```

### Windows

1.  **下载**: 前往 [Releases 页面](https://github.com/你的仓库/releases) 下载最新的 `.msi` 安装包。
2.  **安装**: 双击 `.msi` 文件，按照向导完成安装。
3.  **安全提示**: 首次运行时，如果 Windows SmartScreen 弹出拦截提示，请点击 `更多信息` -> `仍要运行`。
