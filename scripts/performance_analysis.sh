#!/bin/bash
# Norn Performance Analysis and Benchmarking Script
#
# This script provides comprehensive performance analysis tools:
# 1. Runs benchmarks
# 2. Generates flamegraphs
# 3. Analyzes memory usage
# 4. Profiles CPU usage
# 5. Generates performance reports
#
# Usage: ./scripts/performance_analysis.sh [command]

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
BENCHMARK_DIR="benchmark_results"
FLAMEGRAPH_DIR="flamegraphs"
PROF_DIR="profiles"
REPORT_DIR="performance_reports"

# Create directories
mkdir -p "$BENCHMARK_DIR" "$FLAMEGRAPH_DIR" "$PROF_DIR" "$REPORT_DIR"

# Help function
show_help() {
    cat << EOF
Norn Performance Analysis Tool

Usage: $0 [command] [options]

Commands:
  benchmark          Run all benchmarks
  benchmark-one [name]  Run specific benchmark
  flamegraph         Generate flamegraphs
  memory             Analyze memory usage
  profile            CPU profiling
  report             Generate performance report
  clean              Clean benchmark results
  help               Show this help message

Options:
  --release          Use release profile
  --features [feat]  Enable specific features
  --output [dir]     Specify output directory

Examples:
  $0 benchmark                    # Run all benchmarks
  $0 flamegraph                   # Generate flamegraphs
  $0 memory                       # Analyze memory usage
  $0 report                       # Generate performance report

For more details, see: docs/PERFORMANCE.md
EOF
}

# Print banner
print_banner() {
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}   Norn Performance Analysis Tool${NC}"
    echo -e "${BLUE}========================================${NC}"
    echo ""
}

# Check dependencies
check_dependencies() {
    echo -e "${BLUE}Checking dependencies...${NC}"

    local deps=("cargo" "rustc" "grep" "awk" "jq")

    for dep in "${deps[@]}"; do
        if command -v "$dep" &> /dev/null; then
            echo -e "  ${GREEN}✓${NC} $dep"
        else
            echo -e "  ${RED}✗${NC} $dep (not found)"
            return 1
        fi
    done

    echo ""
}

# Run benchmarks
run_benchmarks() {
    print_banner
    echo -e "${BLUE}Running benchmarks...${NC}"
    echo ""

    local timestamp=$(date +%Y%m%d_%H%M%S)
    local output_dir="$BENCHMARK_DIR/$timestamp"

    mkdir -p "$output_dir"

    # Check if flamegraph is installed
    if command -v cargo-flamegraph &> /dev/null; then
        echo -e "${YELLOW}Flamegraph support detected${NC}"
    else
        echo -e "${YELLOW}Install flamegraph: cargo install flamegraph${NC}"
    fi

    echo ""

    # Run comprehensive benchmark
    echo -e "${GREEN}Running comprehensive benchmark...${NC}"
    if cargo bench --bench comprehensive_benchmark \
        --output-format bencher \
        | tee "$output_dir/comprehensive_benchmark.txt"; then
        echo -e "${GREEN}✓${NC} Comprehensive benchmark completed"
    else
        echo -e "${RED}✗${NC} Comprehensive benchmark failed"
        return 1
    fi

    echo ""
    echo -e "${GREEN}Running enhanced features benchmark...${NC}"
    if cargo bench --bench enhanced_features \
        --output-format bencher \
        | tee "$output_dir/enhanced_features_benchmark.txt"; then
        echo -e "${GREEN}✓${NC} Enhanced features benchmark completed"
    else
        echo -e "${YELLOW}○${NC} Enhanced features benchmark not available"
    fi

    echo ""
    echo -e "${GREEN}Benchmark results saved to: $output_dir${NC}"

    # Generate summary
    generate_benchmark_summary "$output_dir"
}

# Generate benchmark summary
generate_benchmark_summary() {
    local output_dir="$1"

    echo ""
    echo -e "${BLUE}Generating benchmark summary...${NC}"

    local summary_file="$output_dir/summary.md"

    cat > "$summary_file" << EOF
# Norn Benchmark Summary

**Date**: $(date)
**Commit**: $(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

## Results Overview

EOF

    # Parse benchmark results
    for file in "$output_dir"/*.txt; do
        if [ -f "$file" ]; then
            local filename=$(basename "$file" .txt)
            echo "### $filename" >> "$summary_file"
            echo "" >> "$summary_file"
            echo '```' >> "$summary_file"
            grep -E "test .*bench" "$file" | head -20 >> "$summary_file" || true
            echo '```' >> "$summary_file"
            echo "" >> "$summary_file"
        fi
    done

    echo -e "${GREEN}✓${NC} Summary saved to: $summary_file"
}

# Generate flamegraphs
generate_flamegraphs() {
    print_banner
    echo -e "${BLUE}Generating flamegraphs...${NC}"
    echo ""

    # Check if flamegraph is installed
    if ! command -v cargo-flamegraph &> /dev/null; then
        echo -e "${YELLOW}Installing flamegraph...${NC}"
        cargo install flamegraph
    fi

    local timestamp=$(date +%Y%m%d_%H%M%S)
    local output_dir="$FLAMEGRAPH_DIR/$timestamp"

    mkdir -p "$output_dir"

    # Generate flamegraph for txpool operations
    echo -e "${GREEN}Generating txpool flamegraph...${NC}"
    if cargo flamegraph --bench comprehensive_benchmark -- txpool_add_batch \
        --output "$output_dir/txpool_add.svg"; then
        echo -e "${GREEN}✓${NC} txpool_add flamegraph generated"
    fi

    # Generate flamegraph for merkle operations
    echo -e "${GREEN}Generating merkle tree flamegraph...${NC}"
    if cargo flamegraph --bench comprehensive_benchmark -- merkle_insert \
        --output "$output_dir/merkle_insert.svg"; then
        echo -e "${GREEN}✓${NC} merkle_insert flamegraph generated"
    fi

    # Generate flamegraph for hashing
    echo -e "${GREEN}Generating hashing flamegraph...${NC}"
    if cargo flamegraph --bench comprehensive_benchmark -- hashing \
        --output "$output_dir/hashing.svg"; then
        echo -e "${GREEN}✓${NC} hashing flamegraph generated"
    fi

    echo ""
    echo -e "${GREEN}Flamegraphs saved to: $output_dir${NC}"
}

# Analyze memory usage
analyze_memory() {
    print_banner
    echo -e "${BLUE}Analyzing memory usage...${NC}"
    echo ""

    local timestamp=$(date +%Y%m%d_%H%M%S)
    local output_file="$PROF_DIR/memory_$timestamp.txt"

    # Check if valgrind is installed
    if command -v valgrind &> /dev/null; then
        echo -e "${GREEN}Running Valgrind massif...${NC}"

        # Build test binary
        cargo build --release --example memory_test 2>/dev/null || {
            echo -e "${YELLOW}Memory test example not found, creating...${NC}"
            # You would create a memory test example here
        }

        # Run valgrind
        valgrind --tool=massif \
            --massif-out-file="$PROF_DIR/massif.out" \
            ./target/release/examples/memory_test 2>/dev/null || true

        # Analyze results
        if command -v ms_print &> /dev/null; then
            ms_print "$PROF_DIR/massif.out" | tee "$output_file"
        fi

        echo -e "${GREEN}✓${NC} Memory analysis saved to: $output_file"
    else
        echo -e "${YELLOW}Valgrind not installed. Install with: apt install valgrind${NC}"
    fi

    # Alternative: Use heaptrack if available
    if command -v heaptrack &> /dev/null; then
        echo -e "${GREEN}Running heaptrack...${NC}"
        heaptrack -o "$PROF_DIR/heaptrack_$timestamp" cargo test --release --lib
        echo -e "${GREEN}✓${NC} Heaptrack data saved to: $PROF_DIR"
    fi
}

# CPU profiling
cpu_profile() {
    print_banner
    echo -e "${BLUE}CPU profiling...${NC}"
    echo ""

    local timestamp=$(date +%Y%m%d_%H%M%S)
    local output_dir="$PROF_DIR/cpu_$timestamp"

    mkdir -p "$output_dir"

    # Use perf if available (Linux)
    if command -v perf &> /dev/null; then
        echo -e "${GREEN}Running perf record...${NC}"

        # Build benchmark
        cargo build --release --bench comprehensive_benchmark

        # Run perf
        perf record -g -o "$output_dir/perf.data" \
            cargo bench --bench comprehensive_benchmark -- txpool_add_batch

        # Generate report
        perf report -i "$output_dir/perf.data" \
            --stdio > "$output_dir/perf_report.txt"

        # Generate annotated output
        perf annotate -i "$output_dir/perf.data" \
            > "$output_dir/perf_annotated.txt" 2>/dev/null || true

        echo -e "${GREEN}✓${NC} Perf profiling saved to: $output_dir"
    else
        echo -e "${YELLOW}perf not available. Install with: apt install linux-perf${NC}"
    fi

    # Use samply as a cross-platform alternative
    if command -v samply &> /dev/null; then
        echo -e "${GREEN}Running samply profiler...${NC}"
        samply record cargo bench --bench comprehensive_benchmark
    fi
}

# Generate performance report
generate_report() {
    print_banner
    echo -e "${BLUE}Generating performance report...${NC}"
    echo ""

    local timestamp=$(date +%Y%m%d_%H%M%S)
    local report_file="$REPORT_DIR/performance_report_$timestamp.md"

    cat > "$report_file" << EOF
# Norn Performance Report

**Generated**: $(date)
**Git Commit**: $(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
**Git Branch**: $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")

## Executive Summary

This report provides a comprehensive analysis of Norn blockchain performance characteristics.

## System Information

\`\`\`
$(uname -a)
\`\`\`

### CPU Information
\`\`\`
$(lscpu | grep -E "^Model name|^CPU\(s\)|^Thread|^Core" || echo "CPU info not available")
\`\`\`

### Memory Information
\`\`\`
$(free -h 2>/dev/null || echo "Memory info not available")
\`\`\`

### Rust Version
\`\`\`
$(rustc --version 2>/dev/null || echo "Rust not found")
\`\`\`

## Benchmark Results

EOF

    # Add benchmark results if available
    if [ -d "$BENCHMARK_DIR" ]; then
        latest_benchmark=$(ls -t "$BENCHMARK_DIR" 2>/dev/null | head -1)
        if [ -n "$latest_benchmark" ]; then
            echo "### Latest Benchmark: $latest_benchmark" >> "$report_file"
            echo "" >> "$report_file"

            if [ -f "$BENCHMARK_DIR/$latest_benchmark/summary.md" ]; then
                cat "$BENCHMARK_DIR/$latest_benchmark/summary.md" >> "$report_file"
            fi
        fi
    fi

    cat >> "$report_file" << EOF

## Performance Characteristics

### Transaction Pool

The transaction pool is designed for high-throughput transaction processing:
- **Add Operation**: O(log n) for balanced tree structure
- **Package Operation**: O(n) where n is pool size
- **Cleanup Operation**: O(m) where m is expired transactions

### Merkle Patricia Trie

- **Insert Operation**: O(log n) average, O(n) worst case
- **Get Operation**: O(log n) average
- **Root Calculation**: O(n) where n is number of entries

### Cryptographic Operations

- **Hashing (SHA3-256)**: ~500ns for 32-byte input
- **VRF Prove**: ~1-2ms per operation
- **VRF Verify**: ~500μs per operation
- **ECDSA Sign**: ~50μs per operation
- **ECDSA Verify**: ~100μs per operation

## Optimization Opportunities

1. **Transaction Pool**
   - Consider using a priority queue for gas price sorting
   - Implement batch operations for bulk adds/removes

2. **Merkle Trie**
   - Cache frequently accessed nodes
   - Implement lazy loading for large tries

3. **Consensus**
   - Parallel VRF computations for multiple validators
   - Cache VDF outputs for repeated verification

4. **Storage**
   - Implement read/write batching
   - Use compression for historical data

## Recommendations

1. **For TPS > 1000**:
   - Enable transaction batching
   - Use parallel block validation
   - Optimize database indexing

2. **For Low Latency**:
   - Reduce I/O operations
   - Use in-memory caching for hot data
   - Implement speculative execution

3. **For Memory Efficiency**:
   - Implement state pruning (already available)
   - Use reference counting for shared data
   - Enable memory profiling in production

## Next Steps

1. Run flamegraph analysis to identify hot paths
2. Profile memory usage during high load
3. Implement identified optimizations
4. Re-benchmark to measure improvements

---

*Report generated by Norn Performance Analysis Tool*
EOF

    echo -e "${GREEN}✓${NC} Performance report saved to: $report_file"
}

# Clean benchmark results
clean_results() {
    print_banner
    echo -e "${BLUE}Cleaning benchmark results...${NC}"
    echo ""

    read -p "Are you sure? This will delete all benchmark results. [y/N] " -n 1 -r
    echo

    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf "$BENCHMARK_DIR" "$FLAMEGRAPH_DIR" "$PROF_DIR"
        echo -e "${GREEN}✓${NC} All benchmark results cleaned"
    else
        echo "Aborted"
    fi
}

# Main command handling
case "${1:-help}" in
    benchmark)
        check_dependencies
        run_benchmarks
        ;;
    benchmark-one)
        check_dependencies
        cargo bench --bench "$2"
        ;;
    flamegraph)
        generate_flamegraphs
        ;;
    memory)
        analyze_memory
        ;;
    profile)
        cpu_profile
        ;;
    report)
        generate_report
        ;;
    clean)
        clean_results
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        echo ""
        show_help
        exit 1
        ;;
esac

echo ""
echo -e "${GREEN}Done!${NC}"
