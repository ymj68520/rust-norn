#!/bin/bash
# 诊断脚本

echo "=== 1. 检查节点进程 ==="
ps aux | grep norn | grep -v grep | head -5

echo -e "\n=== 2. 检查端口 ==="
lsof -i :50051 2>/dev/null || echo "Port 50051 not found with lsof"

echo -e "\n=== 3. 测试 TCP 连接 ==="
timeout 3 bash -c "echo > /dev/tcp/127.0.0.1/50051" 2>/dev/null && echo "✅ Port is accessible" || echo "❌ Port not accessible"

echo -e "\n=== 4. 检查 RPC 服务（使用 grpcurl）==="
if command -v grpcurl &> /dev/null; then
    grpcurl -plaintext 127.0.0.1:50051 list
else
    echo "grpcurl not installed, skipping"
fi

echo -e "\n=== 5. 尝试简单的 RPC 调用 ==="
cat > /tmp/test_rpc.py << 'PYEOF'
import grpc
import sys

# 尝试连接
try:
    channel = grpc.insecure_channel('127.0.0.1:50051')
    grpc.channel_ready_future(channel).result(timeout=5)
    print("✅ gRPC channel ready")
except Exception as e:
    print(f"❌ gRPC connection failed: {e}")
    sys.exit(1)
PYEOF

python3 /tmp/test_rpc.py 2>&1

echo -e "\n=== 6. 查找并检查节点日志 ==="
find /tmp -name "*norn*.log" -o -name "node*.log" 2>/dev/null | head -5 | while read log; do
    echo "Found log: $log"
    tail -20 "$log"
done
