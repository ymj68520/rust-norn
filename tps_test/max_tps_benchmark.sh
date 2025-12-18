#!/bin/bash

# TPS 压力测试脚本 - 寻找最大TPS
# 通过阶梯式测试找出系统的最大承载能力

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# 配置
RPC_ADDRESS="${RPC_ADDRESS:-127.0.0.1:50051}"
TEST_DURATION="${TEST_DURATION:-60}"  # 每个测试的持续时间
WAIT_TIME="${WAIT_TIME:-30}"          # 等待打包时间
BATCH_SIZE="${BATCH_SIZE:-50}"        # 批次大小
RESULTS_DIR="tps_test_results"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
RESULT_FILE="$RESULTS_DIR/benchmark_$TIMESTAMP.csv"

# 测试配置
declare -a TPS_LEVELS=(
    "100"      # 低负载基准
    "250"      # 低负载
    "500"      # 中低负载
    "750"      # 中等负载
    "1000"     # 中高负载
    "1500"     # 高负载
    "2000"     # 很高负载
    "3000"     # 极高负载
    "5000"     # 压力测试
    "7500"     # 严重压力
    "10000"    # 极限压力
)

# 打印函数
print_header() {
    echo -e "${CYAN}╔════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║${BOLD}            $1${NC}${CYAN}                                    ║${NC}"
    echo -e "${CYAN}╚════════════════════════════════════════════════════════════╝${NC}"
}

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

print_test_start() {
    echo -e "${MAGENTA}🚀 $1${NC}"
}

print_result() {
    echo -e "${BOLD}📊 $1${NC}"
}

# 创建结果目录
mkdir -p "$RESULTS_DIR"

# 初始化结果文件
echo "test_rate,submitted,failed,success_rate%,packed,actual_tps,achievement_rate%,duration_sec" > "$RESULT_FILE"

# 检查节点
check_node() {
    local host=$(echo "$RPC_ADDRESS" | cut -d: -f1)
    local port=$(echo "$RPC_ADDRESS" | cut -d: -f2)

    # 方法1: 使用 /dev/tcp
    if timeout 2 bash -c "echo > /dev/tcp/$host/$port" 2>/dev/null; then
        return 0
    fi

    # 方法2: 使用 nc
    if nc -z "$host" "$port" 2>/dev/null; then
        return 0
    fi

    return 1
}

# 运行单次测试
run_tps_test() {
    local target_tps=$1
    local test_num=$2
    local total_tests=$3

    echo ""
    print_header "测试 $test_num/$total_tests: ${target_tps} TPS"

    local log_file="$RESULTS_DIR/test_${target_tps}_tps_${TIMESTAMP}.log"

    print_info "测试配置:"
    echo "   目标 TPS: $target_tps"
    echo "   测试时长: ${TEST_DURATION} 秒"
    echo "   批次大小: $BATCH_SIZE"
    echo "   RPC 地址: $RPC_ADDRESS"
    echo "   日志文件: $log_file"
    echo ""

    print_test_start "开始测试..."

    # 运行测试并捕获输出
    if /home/ymj68520/projects/Rust/rust-norn/target/release/tps_test \
        --rpc-address "$RPC_ADDRESS" \
        --duration "$TEST_DURATION" \
        --rate "$target_tps" \
        --batch-size "$BATCH_SIZE" \
        2>&1 | tee "$log_file"; then

        print_success "测试完成"

        # 从日志中提取结果
        local submitted=$(grep "已提交:" "$log_file" | tail -1 | awk '{print $2}')
        local failed=$(grep "失败:" "$log_file" | tail -1 | awk '{print $2}')
        local packed=$(grep "打包交易:" "$log_file" | tail -1 | awk '{print $2}')
        local actual_tps=$(grep "实际 TPS:" "$log_file" | tail -1 | awk '{print $3}')
        local achievement=$(grep "达成率:" "$log_file" | tail -1 | awk '{print $2}' | tr -d '%')
        local success_rate=$(grep "成功率:" "$log_file" | tail -1 | awk '{print $2}' | tr -d '%')

        # 计算提交速率
        local submit_tps=$(awk "BEGIN {printf \"%.2f\", $submitted / $TEST_DURATION}")

        # 保存结果
        echo "$target_tps,$submitted,$failed,${success_rate:-0},$packed,${actual_tps:-0},${achievement:-0},$TEST_DURATION" >> "$RESULT_FILE"

        # 显示摘要
        echo ""
        print_result "测试摘要:"
        echo "   📦 提交: $submitted 笔 | 失败: $failed 笔 | 成功率: ${success_rate:-0}%"
        echo "   ⛓️  打包: $packed 笔 | 实际 TPS: ${actual_tps:-0} | 达成率: ${achievement:-0}%"
        echo "   📈 提交速率: $submit_tps TPS"

        # 判断是否达到阈值
        local ach_num=$(echo "$achievement" | awk '{printf "%d", $1}')
        if [ "$ach_num" -ge 90 ]; then
            print_success "性能优秀: 达成率 ${achievement:-0}% >= 90%"
            return 0  # 继续测试
        elif [ "$ach_num" -ge 70 ]; then
            print_warning "性能良好: 达成率 ${achievement:-0}% >= 70%"
            return 0  # 继续测试
        elif [ "$ach_num" -ge 50 ]; then
            print_warning "性能一般: 达成率 ${achievement:-0}% >= 50%，接近瓶颈"
            return 0  # 继续测试，但可能快到极限了
        else
            print_error "性能不佳: 达成率 ${achievement:-0}% < 50%，已达到瓶颈"
            return 1  # 达到瓶颈，停止测试
        fi
    else
        print_error "测试失败"
        echo "$target_tps,0,0,0,0,0,0,$TEST_DURATION" >> "$RESULT_FILE"
        return 1
    fi
}

# 主函数
main() {
    print_header "TPS 最大性能压力测试"

    print_info "测试配置:"
    echo "   RPC 地址: $RPC_ADDRESS"
    echo "   测试时长: ${TEST_DURATION} 秒/级别"
    echo "   等待时间: ${WAIT_TIME} 秒"
    echo "   批次大小: $BATCH_SIZE"
    echo "   结果目录: $RESULTS_DIR"
    echo ""

    # 检查节点
    print_info "检查节点状态..."
    if check_node; then
        print_success "节点正在运行 ($RPC_ADDRESS)"
    else
        print_error "节点未运行，请先启动节点"
        exit 1
    fi

    # 检查二进制文件
    if [ ! -f "./target/release/tps_test" ]; then
        print_warning "测试工具未编译，正在编译..."
        cargo build -p tps_test --release
        print_success "编译完成"
    fi

    echo ""
    print_info "测试计划:"
    echo "   将按以下TPS级别进行阶梯式测试："
    for i in "${!TPS_LEVELS[@]}"; do
        echo "   [$((i+1))] ${TPS_LEVELS[$i]} TPS"
    done
    echo ""

    read -p "按 Enter 开始测试，或 Ctrl+C 取消..."

    # 执行测试
    local total_tests=${#TPS_LEVELS[@]}
    local completed=0
    local max_sustainable_tps=0
    local max_tested_tps=0

    for tps in "${TPS_LEVELS[@]}"; do
        if run_tps_test "$tps" $((completed + 1)) "$total_tests"; then
            completed=$((completed + 1))
            max_tested_tps=$tps

            # 如果达成率 >= 70%，认为是可持续的
            local ach=$(grep "^${tps}," "$RESULT_FILE" | tail -1 | cut -d',' -f7 | awk '{printf "%d", $1}')
            if [ "$ach" -ge 70 ]; then
                max_sustainable_tps=$tps
            fi

            # 短暂休息，让系统恢复
            if [ $completed -lt $total_tests ]; then
                echo ""
                print_info "休息 10 秒后继续下一个测试..."
                sleep 10
            fi
        else
            print_warning "达到性能瓶颈，停止测试"
            break
        fi
    done

    # 生成报告
    echo ""
    print_header "测试完成 - 最终报告"

    print_result "测试统计:"
    echo "   完成测试: $completed / $total_tests"
    echo "   最大测试 TPS: $max_tested_tps"
    echo "   最大可持续 TPS (达成率>=70%): $max_sustainable_tps"
    echo ""

    print_result "详细结果:"
    echo ""
    column -t -s',' "$RESULT_FILE" | while IFS=' ' read -r line; do
        if [[ $line =~ ^test_rate ]]; then
            echo -e "${BOLD}$line${NC}"
        else
            echo "$line"
        fi
    done

    echo ""
    print_result "性能分析:"

    # 找出最佳性能点
    local best_tps=0
    local best_achievement=0
    while IFS=',' read -r rate submitted failed success_rate packed actual_tps achievement duration; do
        if [ "$rate" = "test_rate" ]; then
            continue
        fi

        ach_num=$(echo "$achievement" | awk '{printf "%d", $1}')
        if [ $ach_num -ge 70 ] && [ $ach_num -gt $best_achievement ]; then
            best_achievement=$ach_num
            best_tps=$rate
        fi
    done < "$RESULT_FILE"

    if [ $best_tps -gt 0 ]; then
        print_success "推荐生产配置: $best_tps TPS (达成率: ${best_achievement}%)"
    fi

    if [ $max_sustainable_tps -gt 0 ]; then
        print_success "最大可持续 TPS: $max_sustainable_tps"
    fi

    if [ $max_tested_tps -gt $max_sustainable_tps ]; then
        print_warning "理论最大 TPS: $max_tested_tps (性能已下降)"
    fi

    echo ""
    print_result "文件位置:"
    echo "   CSV 结果: $RESULT_FILE"
    echo "   详细日志: $RESULTS_DIR/"

    # 生成可视化报告
    echo ""
    print_info "生成性能图表..."

    # 创建简单的ASCII图表
    local chart_file="$RESULTS_DIR/performance_chart_${TIMESTAMP}.txt"
    {
        echo "TPS 性能测试结果图表"
        echo "====================="
        echo ""
        printf "%-10s %-15s %-15s %-15s\n" "TPS" "实际TPS" "达成率" "状态"
        echo "--------------------------------------------------------"

        while IFS=',' read -r rate submitted failed success_rate packed actual_tps achievement duration; do
            if [ "$rate" = "test_rate" ]; then
                continue
            fi

            local status="未知"
            local ach_num=$(echo "$achievement" | awk '{printf "%d", $1}')

            if [ $ach_num -ge 90 ]; then
                status="✅ 优秀"
            elif [ $ach_num -ge 70 ]; then
                status="🟡 良好"
            elif [ $ach_num -ge 50 ]; then
                status="🟠 一般"
            else
                status="🔴 差"
            fi

            printf "%-10s %-15s %-15s %-15s\n" \
                "$rate" \
                "${actual_tps:-0}" \
                "${achievement:-0}%" \
                "$status"
        done < "$RESULT_FILE"

        echo ""
        echo "图例:"
        echo "  ✅ 优秀 - 达成率 >= 90%"
        echo "  🟡 良好 - 达成率 >= 70%"
        echo "  🟠 一般 - 达成率 >= 50%"
        echo "  🔴 差   - 达成率 < 50%"
    } > "$chart_file"

    print_success "图表已保存: $chart_file"

    echo ""
    print_header "测试完成！"
}

# 运行
main "$@"
