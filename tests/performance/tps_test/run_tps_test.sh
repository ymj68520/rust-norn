#!/bin/bash

# TPS 测试脚本
# 用于自动化运行 TPS 性能测试

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 打印带颜色的消息
print_info() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

# 检查二进制文件是否存在
check_binary() {
    if [ ! -f "./target/release/tps_test" ]; then
        print_warning "TPS 测试工具未编译，正在编译..."
        cargo build -p tps_test --release
        print_success "编译完成"
    fi
}

# 检查节点是否运行
check_node() {
    local rpc_address=$1
    local host=$(echo $rpc_address | cut -d: -f1)
    local port=$(echo $rpc_address | cut -d: -f2)

    if nc -z "$host" "$port" 2>/dev/null; then
        return 0
    else
        return 1
    fi
}

# 主函数
main() {
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║           TPS 性能测试自动化脚本                           ║"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""

    # 解析参数
    RPC_ADDRESS="${RPC_ADDRESS:-127.0.0.1:50051}"
    DURATION="${DURATION:-60}"
    RATE="${RATE:-100}"
    BATCH_SIZE="${BATCH_SIZE:-10}"

    print_info "测试配置:"
    echo "   RPC 地址: $RPC_ADDRESS"
    echo "   测试时长: $DURATION 秒"
    echo "   目标 TPS: $RATE"
    echo "   批次大小: $BATCH_SIZE"
    echo ""

    # 检查二进制文件
    check_binary

    # 检查节点是否运行
    print_info "检查节点状态..."
    if check_node "$RPC_ADDRESS"; then
        print_success "节点正在运行 ($RPC_ADDRESS)"
    else
        print_error "节点未运行 ($RPC_ADDRESS)"
        echo ""
        echo "请先启动节点："
        echo "  方式 1: cd docker && ./start-nodes.sh"
        echo "  方式 2: ./target/release/norn --config config.toml"
        exit 1
    fi

    echo ""
    print_info "开始 TPS 测试..."
    echo "╔════════════════════════════════════════════════════════════╗"
    echo ""

    # 运行测试
    ./target/release/tps_test \
        --rpc-address "$RPC_ADDRESS" \
        --duration "$DURATION" \
        --rate "$RATE" \
        --batch-size "$BATCH_SIZE"

    TEST_EXIT_CODE=$?

    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    echo ""

    if [ $TEST_EXIT_CODE -eq 0 ]; then
        print_success "测试完成!"
    else
        print_error "测试失败 (退出码: $TEST_EXIT_CODE)"
        exit $TEST_EXIT_CODE
    fi
}

# 显示帮助信息
show_help() {
    cat << EOF
使用方法:
    $0 [OPTIONS]

环境变量:
    RPC_ADDRESS    RPC 服务器地址 (默认: 127.0.0.1:50051)
    DURATION       测试持续时间（秒） (默认: 60)
    RATE           目标 TPS (默认: 100)
    BATCH_SIZE     批次大小 (默认: 10)

示例:
    # 使用默认配置
    $0

    # 自定义参数
    RPC_ADDRESS=127.0.0.1:50051 RATE=500 DURATION=120 $0

    # 压力测试
    RATE=1000 DURATION=300 BATCH_SIZE=50 $0

EOF
}

# 处理命令行参数
case "${1:-}" in
    -h|--help|help)
        show_help
        exit 0
        ;;
    *)
        main
        ;;
esac
