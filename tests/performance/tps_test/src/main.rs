mod transaction_generator;
mod rpc_client;
mod statistics;

use clap::Parser;
use anyhow::Result;
use tracing::{info, warn, error};
use tracing_subscriber;
use std::time::Duration;
use tokio::time::Instant;

use transaction_generator::TransactionGenerator;
use rpc_client::BlockchainRpcClient;
use statistics::TestStatistics;

/// TPS 测试配置
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// RPC 服务器地址 (例如: 127.0.0.1:50051)
    #[arg(short, long, default_value = "127.0.0.1:50051")]
    rpc_address: String,

    /// 测试持续时间（秒）
    #[arg(short, long, default_value_t = 60)]
    duration: u64,

    /// 目标 TPS（每秒交易数）
    #[arg(short = 'r', long, default_value_t = 100)]
    rate: u64,

    /// 并发连接数
    #[arg(short = 'c', long, default_value_t = 10)]
    concurrent: usize,

    /// 每批交易数量
    #[arg(short = 'b', long, default_value_t = 10)]
    batch_size: usize,

    /// 监控间隔（秒）
    #[arg(short = 'i', long, default_value_t = 5)]
    monitor_interval: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();

    info!("🚀 启动 TPS 性能测试");
    info!("=======================");
    info!("RPC 地址: {}", args.rpc_address);
    info!("测试持续时间: {} 秒", args.duration);
    info!("目标 TPS: {}", args.rate);
    info!("并发连接数: {}", args.concurrent);
    info!("批次大小: {}", args.batch_size);
    info!("监控间隔: {} 秒", args.monitor_interval);
    info!("=======================");

    // 连接到 RPC 服务器
    info!("📡 连接到 RPC 服务器: {}", args.rpc_address);
    let mut client = BlockchainRpcClient::connect(&args.rpc_address).await?;
    info!("✅ 成功连接到 RPC 服务器");

    // 获取初始区块信息
    let initial_block_number = client.get_block_number().await?;
    info!("📊 当前区块高度: {}", initial_block_number);

    // 初始化交易生成器
    let mut generator = TransactionGenerator::new();

    // 初始化统计跟踪器
    let mut stats = TestStatistics::new(args.rate, initial_block_number);

    // 计算批次间隔时间
    let batches_per_sec = (args.rate as f64 / args.batch_size as f64).max(0.1);
    let batch_interval = Duration::from_millis((1000.0 / batches_per_sec) as u64);

    info!("🎯 开始发送交易（目标 TPS: {}）", args.rate);
    info!("⏱️  批次间隔: {:?}", batch_interval);

    let test_start = Instant::now();
    let test_duration = Duration::from_secs(args.duration);

    // 交易发送循环
    let mut total_sent = 0u64;
    let mut batch_count = 0u64;

    while test_start.elapsed() < test_duration {
        let batch_start = Instant::now();

        // 生成并发送一批交易
        for _ in 0..args.batch_size {
            let tx = generator.generate_random_transaction();

            let send_time = chrono::Utc::now().timestamp_millis();
            match client.send_transaction_with_data(&tx).await {
                Ok(_tx_hash) => {
                    stats.track_submission(send_time);
                    total_sent += 1;

                    if total_sent % 1000 == 0 {
                        info!("📦 已发送 {} 笔交易", total_sent);
                    }
                }
                Err(e) => {
                    error!("❌ 发送交易失败: {} | 原因: {}", e, e.root_cause());
                    stats.track_failed_submission();
                }
            }
        }

        batch_count += 1;

        // 定期监控进度
        if batch_count % (args.monitor_interval * 1000 / batch_interval.as_millis() as u64) == 0 {
            let elapsed = test_start.elapsed().as_secs_f64();
            let current_tps = total_sent as f64 / elapsed;
            info!("📈 进度报告:");
            info!("   已发送: {} 笔交易", total_sent);
            info!("   当前速率: {:.2} TPS", current_tps);
            info!("   已用时间: {:.1} 秒", elapsed);
        }

        // 等待下一个批次
        let elapsed = batch_start.elapsed();
        if elapsed < batch_interval {
            tokio::time::sleep(batch_interval - elapsed).await;
        }
    }

    let send_duration = test_start.elapsed();
    info!("✅ 交易发送完成!");
    info!("   总发送: {} 笔交易", total_sent);
    info!("   发送耗时: {:?}", send_duration);
    info!("   发送速率: {:.2} TPS", total_sent as f64 / send_duration.as_secs_f64());

    // 等待一段时间让所有交易被打包
    info!("⏳ 等待交易打包（30秒）...");
    tokio::time::sleep(Duration::from_secs(30)).await;

    // 监控区块链以计算实际 TPS
    info!("🔍 开始监控区块链打包情况...");
    monitor_blockchain(&mut client, &mut stats, initial_block_number).await?;

    // 打印最终统计报告
    info!("\n");
    info!("╔════════════════════════════════════════════════════════════╗");
    info!("║                    TPS 测试报告                              ║");
    info!("╚════════════════════════════════════════════════════════════╝");
    stats.print_report();
    info!("╔════════════════════════════════════════════════════════════╗");
    info!("║                        测试完成                              ║");
    info!("╚════════════════════════════════════════════════════════════╝");

    Ok(())
}

/// 监控区块链并统计实际 TPS
async fn monitor_blockchain(
    client: &mut BlockchainRpcClient,
    stats: &mut TestStatistics,
    start_block: i64,
) -> Result<()> {
    let current_block = client.get_block_number().await?;
    info!("📊 当前区块高度: {}", current_block);

    let mut total_transactions = 0u64;
    let mut total_blocks = 0u64;
    let mut start_timestamp: Option<i64> = None;
    let mut end_timestamp: Option<i64> = None;

    // 遍历所有新区块
    for height in start_block..=current_block {
        match client.get_block_by_number(height).await {
            Ok(Some(block)) => {
                let tx_count = block.transactions.len() as u64;
                total_transactions += tx_count;
                total_blocks += 1;

                let timestamp = block.header.timestamp;
                if start_timestamp.is_none() {
                    start_timestamp = Some(timestamp);
                }
                end_timestamp = Some(timestamp);

                if total_blocks % 10 == 0 {
                    info!("   已处理 {} 个区块，共 {} 笔交易", total_blocks, total_transactions);
                }
            }
            Ok(None) => {
                warn!("⚠️  区块 {} 未找到", height);
            }
            Err(e) => {
                error!("❌ 获取区块 {} 失败: {}", height, e);
            }
        }
    }

    // 计算实际 TPS
    if let (Some(start_ts), Some(end_ts)) = (start_timestamp, end_timestamp) {
        let time_span = (end_ts - start_ts).max(1) as f64 / 1000.0; // 转换为秒
        let actual_tps = if time_span > 0.0 {
            total_transactions as f64 / time_span
        } else {
            0.0
        };

        stats.set_blockchain_metrics(
            total_transactions,
            total_blocks,
            actual_tps,
            time_span,
        );
    }

    Ok(())
}
