#!/bin/bash

# Norn 项目统一测试运行脚本
# 从项目根目录运行所有测试

set -e

# 颜色定义
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}====================================${NC}"
echo -e "${BLUE}  Norn 测试套件${NC}"
echo -e "${BLUE}====================================${NC}"
echo ""

# 检查是否在项目根目录
if [ ! -f "Cargo.toml" ] || [ ! -d "tests" ]; then
    echo "错误: 请在项目根目录运行此脚本"
    exit 1
fi

# 进入测试目录
cd tests

# 运行统一测试脚本
if [ -f "run_all_tests.sh" ]; then
    chmod +x run_all_tests.sh
    ./run_all_tests.sh
else
    echo "错误: 测试脚本不存在"
    exit 1
fi
