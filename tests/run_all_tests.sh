#!/bin/bash

# Norn 统一测试脚本
# 运行所有测试并生成报告

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 统计变量
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0
FAILED_TEST_NAMES=()

# 打印带颜色的消息
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
}

print_error() {
    echo -e "${RED}[FAIL]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# 打印标题
print_header() {
    echo ""
    echo -e "${BLUE}====================================${NC}"
    echo -e "${BLUE} $1${NC}"
    echo -e "${BLUE}====================================${NC}"
    echo ""
}

# 运行测试并记录结果
run_test() {
    local test_name=$1
    local test_command=$2

    ((TOTAL_TESTS++))

    print_info "Running: $test_name"

    if eval "$test_command" > /tmp/test_output.log 2>&1; then
        print_success "$test_name"
        ((PASSED_TESTS++))
        return 0
    else
        print_error "$test_name"
        ((FAILED_TESTS++))
        FAILED_TEST_NAMES+=("$test_name")
        print_warning "Check /tmp/test_output.log for details"
        return 1
    fi
}

# 切换到项目根目录
cd "$(dirname "$0")/.."

print_header "Norn 测试套件"

# 检查环境
print_info "检查环境..."
if ! command -v cargo &> /dev/null; then
    print_error "cargo 未安装"
    exit 1
fi

# 构建工作空间
print_info "构建测试工作空间..."
cd tests
cargo build --workspace 2>&1 | grep -v "warning:" || true

# 运行单元测试
print_header "单元测试"
run_test "单元测试 (unit)" "cargo test --workspace -p unit-test"

# 运行集成测试
print_header "集成测试"
run_test "集成测试 (integration)" "cargo test --workspace -p integration-test"

# 运行可扩展性测试
print_header "可扩展性测试"
run_test "可扩展性测试 (scalability)" "cargo test --workspace -p scalability-test"

# 运行 E2E 测试
print_header "端到端测试"
cd e2e
if [ -f "e2e_full_workflow_test.rs" ]; then
    run_test "E2E 完整流程测试" "cargo test --test e2e_full_workflow_test"
fi
if [ -f "integration_test.rs" ]; then
    run_test "E2E 集成测试" "cargo test --test integration_test"
fi
cd ..

# 性能测试（可选）
print_header "性能测试"
read -p "是否运行性能测试？(TPS测试可能需要较长时间) [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    print_info "构建 TPS 测试..."
    cd performance/tps_test
    cargo build --release 2>&1 | grep -v "warning:" || true

    print_info "运行 TPS 测试（100 TPS, 10秒）..."
    timeout 30 ./target/release/tps_test --rate 100 --duration 10 || \
        print_warning "TPS 测试超时或失败"

    cd ../..
fi

# 打印总结
print_header "测试总结"
echo "总测试数: $TOTAL_TESTS"
echo -e "${GREEN}通过: $PASSED_TESTS${NC}"
echo -e "${RED}失败: $FAILED_TESTS${NC}"

if [ $FAILED_TESTS -gt 0 ]; then
    echo ""
    print_error "失败的测试:"
    for name in "${FAILED_TEST_NAMES[@]}"; do
        echo "  - $name"
    done
    echo ""
    print_error "存在测试失败！"
    exit 1
else
    echo ""
    print_success "所有测试通过！"
    exit 0
fi
