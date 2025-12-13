#!/bin/bash

# 脚本用于从模板生成config.toml文件
# 使用环境变量替换模板中的占位符

set -e

TEMPLATE_FILE="src-tauri/config.toml.template"
OUTPUT_FILE="src-tauri/config.toml"

# 检查模板文件是否存在
if [ ! -f "$TEMPLATE_FILE" ]; then
    echo "错误: 模板文件 $TEMPLATE_FILE 不存在"
    exit 1
fi

# 设置默认值
MONGODB_REMOTE_URI=${MONGODB_REMOTE_URI:-"mongodb+srv://default:password@cluster.mongodb.net/db"}
PYTHON_WORKING_DIR=${PYTHON_WORKING_DIR:-"/app"}
BASH_WORKING_DIR=${BASH_WORKING_DIR:-"/app"}

# 创建临时文件
TEMP_FILE=$(mktemp)

# 替换环境变量
export MONGODB_REMOTE_URI PYTHON_WORKING_DIR BASH_WORKING_DIR
envsubst < "$TEMPLATE_FILE" > "$TEMP_FILE"

# 移动到最终位置
mv "$TEMP_FILE" "$OUTPUT_FILE"

echo "配置文件已生成: $OUTPUT_FILE"