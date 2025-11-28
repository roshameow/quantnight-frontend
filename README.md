## 首次安装与配置

要成功运行此应用，您需要准备好 MongoDB 数据库并提供一个配置文件。

### 1. MongoDB 环境准备

本应用需要一个正在运行的 MongoDB 实例。请确保您已创建了以下三个数据库：

1.  `simulation_mission`
2.  `simulation_db`
3.  `alpha_db`

**关键步骤：** 在 `simulation_mission` 数据库中，您**必须**手动创建一个名为 `tasks` 的集合，否则应用将无法正常启动。

### 2. 创建 `config.toml` 配置文件

应用通过 `config.toml` 文件来获取数据库连接信息和其他配置。请在项目的 `src-tauri` 目录下创建一个名为 `config.toml` 的文件，并填入以下内容。

**`config.toml` 示例:**
```toml
# ... (其他配置，如 [python] 和 [bash])

[mongodb]
# 本地 MongoDB 连接字符串
local_uri = "mongodb://localhost:27017"

# 远程 MongoDB 连接字符串 (用于远程任务)
remote_uri = "mongodb://user:password@remote_host:27017"

# [mongodb.databases]
# 定义后端代码使用的逻辑名称与实际数据库名称的映射
# 您可以修改右侧的实际数据库名，只要左侧的键 (mission, simulation, alpha) 保持不变
mission = "simulation_mission"
simulation = "simulation_db"
alpha = "alpha_db"
```

-   **`local_uri`**: 指向您的主要 MongoDB 实例。
-   **`remote_uri`**: 如果您需要执行远程任务，请配置此项；否则可保留为空或指向本地实例。
-   **`[mongodb.databases]`**: 这个表格将代码中使用的名称（例如 `mission`）映射到您在 MongoDB 中创建的实际数据库名称。这为您提供了灵活性，例如，您可以将 `alpha` 数据库在 MongoDB 中命名为 `my_alpha_results_db`，只需在此文件中进行相应修改即可。

## 🧭 安装说明（macOS）

### 📦 安装步骤

1. **下载应用**

   前往 [Releases 页面](https://github.com/你的仓库/releases) 下载最新版本的 `quantnight-frontend.dmg` 或 `.zip` 安装包。

2. **安装应用**

   解压后，将 `quantnight-frontend.app` 拖动到 `/Applications` 目录中。

3. **首次打开提示：“app已损坏, 无法打开。 你应该将它移到废纸篓”**

   是因为macOS 默认阻止未签名应用运行。你可以通过以下任一方式解决：

---

### 🛡️ 绕过 macOS 的验证机制

#### ✅ 方法 1：使用命令行（推荐）

在终端中运行以下命令移除 App 的隔离标记：

```bash
sudo xattr -r -d com.apple.quarantine /Applications/quantnight-frontend.app
```

该命令会移除“隔离区”属性，允许应用被正常识别和打开。

#### ✅ 方法 2：系统设置中手动放行

1. 打开 `系统设置 > 隐私与安全性`
2. 滚动到页面底部，找到 **“quantnight-frontend.app 被阻止使用”**
3. 点击 **“仍要打开”**

---

### 🚀 启动应用

完成上述操作后，双击 `/Applications/quantnight-frontend.app` 即可启动。macOS Ventura 及以上版本可能仍会提示一次“是否打开”，点击“打开”即可。


## 🧭 安装说明（Windows）
◊
### 📦 安装步骤

1. **下载应用**

    前往 [Releases 页面](https://github.com/你的仓库/releases) 下载最新版本的 Windows 安装包（`.msi` 或 `.exe` 格式）。

2. **安装应用**

    双击下载的 `.exe` 或 `.msi` 文件，按照提示完成安装。

3. **启动应用**

    安装完成后，可在“开始菜单”中搜索并启动 `quantnight-frontend`。

---

### 🔒 Windows SmartScreen 拦截提示（如遇）

首次运行时，Windows 可能会弹出 SmartScreen 提示：

> “Windows 已保护您的电脑 – 来自未知发布者的应用可能对电脑有害。”

#### ✅ 解决方法

1. 点击 **“更多信息”**
2. 点击 **“仍要运行”**

这是 Windows 针对未签名程序的安全保护机制，绕过后不会影响应用的正常使用。