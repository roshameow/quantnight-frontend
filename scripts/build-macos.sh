#!/bin/bash

set -e

APP_NAME="quantnight-frontend"
APP_DIR="src-tauri/target/release/bundle/macos/${APP_NAME}.app"
EXEC_PATH="$APP_DIR/Contents/MacOS/$APP_NAME"
FRAMEWORKS_DIR="$APP_DIR/Contents/Frameworks"
LIBICONV_SRC="$(brew --prefix libiconv)/lib/libiconv.2.dylib"

echo "📦 [1/6] 开始构建 Tauri 应用..."
npm run tauri build

echo "📁 [2/6] 确保 .app 的 Frameworks 目录存在..."
mkdir -p "$FRAMEWORKS_DIR"

echo "📥 [3/6] 拷贝 libiconv.2.dylib 到 .app Frameworks 中..."
if [ ! -f "$LIBICONV_SRC" ]; then
  echo "❌ 找不到 libiconv.2.dylib，请确认是否安装了 libiconv"
  exit 1
fi
cp "$LIBICONV_SRC" "$FRAMEWORKS_DIR"

echo "🛠️ [4/6] 添加 rpath 到可执行文件..."
install_name_tool -add_rpath "@executable_path/../Frameworks" "$EXEC_PATH" || echo "⚠️ rpath 已存在，跳过添加"

echo "🔧 [5/6] 修改可执行文件对 libiconv 的依赖路径..."
install_name_tool -change "$LIBICONV_SRC" "@rpath/libiconv.2.dylib" "$EXEC_PATH"

echo "🪪 [6/6] 修改 libiconv.2.dylib 自身 install_name（可选）..."
install_name_tool -id "@rpath/libiconv.2.dylib" "$FRAMEWORKS_DIR/libiconv.2.dylib"

echo "✅ 构建完成！可执行文件位置："
echo "$APP_DIR"
