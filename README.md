

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

