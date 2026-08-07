mod rpc_client;
mod statistics;
mod transaction_generator;

use anyhow::Result;
use clap::Parser;
use rayon::prelude::*;
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::Instant;
use tracing::{error, info, warn};
use tracing_subscriber;

use rpc_client::BlockchainRpcClient;
use statistics::TestStatistics;
use transaction_generator::V2TransactionGenerator;

fn generate_v2_batch_parallel(
    generators: &mut [V2TransactionGenerator],
    count: usize,
) -> Vec<norn_common::types::TransactionV2> {
    let signer_count = generators.len();
    generators
        .par_iter_mut()
        .enumerate()
        .map(|(index, generator)| {
            let lane_count = count / signer_count + usize::from(index < count % signer_count);
            (0..lane_count)
                .map(|_| generator.generate_random_transaction())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect()
}

/// TPS 测试配置
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// RPC server address, or comma-separated validator addresses for round-robin batches.
    #[arg(short = 'a', long, default_value = "127.0.0.1:50051")]
    rpc_address: String,

    /// Stable signer slot for this validator's benchmark stream.
    #[arg(long, default_value_t = 1)]
    node_id: u8,

    /// Independent sender lanes. Each lane submits nonces serially; batches
    /// can still use concurrent RPCs across different senders.
    #[arg(long, default_value_t = 1)]
    signers: usize,

    /// Next nonce for every independent sender lane. Keep zero for a fresh
    /// chain; set it to the finalized per-lane nonce when running another
    /// benchmark against the same chain.
    #[arg(long, default_value_t = 0)]
    start_nonce: u64,

    /// Comma-separated Ethereum RPC endpoints used only for pre-consensus bootstrap funding.
    /// Leave empty for normal BFT benchmarks; benchmark accounts must then be pre-funded.
    #[arg(long, value_delimiter = ',')]
    faucet_peers: Vec<String>,

    /// Fund the configured signer lanes on every faucet peer and exit before
    /// sending benchmark traffic. This is intended for fresh devnet setup
    /// before the first BFT finality.
    #[arg(long)]
    bootstrap_only: bool,

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

    /// Maximum time to wait for accepted transactions to appear in canonical blocks.
    #[arg(long, default_value_t = 180)]
    settle_timeout: u64,

    /// Sign the complete workload before submission timing begins.
    #[arg(long)]
    presign: bool,

    /// Submit each batch to every configured validator while counting it once.
    /// Intended for preloaded consensus-ceiling tests with identical pools.
    #[arg(long)]
    replicate_endpoints: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();
    if args.duration == 0 {
        anyhow::bail!("--duration must be greater than zero");
    }
    if args.rate == 0 {
        anyhow::bail!("--rate must be greater than zero");
    }
    if args.concurrent == 0 {
        anyhow::bail!("--concurrent must be greater than zero");
    }
    if args.signers == 0 || args.signers > u8::MAX as usize {
        anyhow::bail!("--signers must be between 1 and {}", u8::MAX);
    }
    if args.batch_size == 0 {
        anyhow::bail!("--batch-size must be greater than zero");
    }
    if args.batch_size > norn_common::types::TransactionV2Batch::MAX_TRANSACTIONS {
        anyhow::bail!(
            "--batch-size must not exceed {}",
            norn_common::types::TransactionV2Batch::MAX_TRANSACTIONS
        );
    }
    if args.monitor_interval == 0 {
        anyhow::bail!("--monitor-interval must be greater than zero");
    }
    if args.settle_timeout == 0 {
        anyhow::bail!("--settle-timeout must be greater than zero");
    }
    if args.bootstrap_only && args.faucet_peers.is_empty() {
        anyhow::bail!("--bootstrap-only requires at least one --faucet-peers endpoint");
    }

    info!("🚀 启动 TPS 性能测试");
    info!("=======================");
    info!("RPC 地址: {}", args.rpc_address);
    info!("测试持续时间: {} 秒", args.duration);
    info!("目标 TPS: {}", args.rate);
    info!("并发连接数: {}", args.concurrent);
    info!("Benchmark signer lanes: {}", args.signers);
    info!("批次大小: {}", args.batch_size);
    info!("监控间隔: {} 秒", args.monitor_interval);
    info!("=======================");

    // 连接到 RPC 服务器
    info!("📡 连接到 RPC 服务器: {}", args.rpc_address);
    let rpc_addresses = args
        .rpc_address
        .split(',')
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .collect::<Vec<_>>();
    if rpc_addresses.is_empty() {
        anyhow::bail!("--rpc-address must contain at least one endpoint");
    }
    let mut submission_clients = Vec::with_capacity(rpc_addresses.len());
    for address in &rpc_addresses {
        submission_clients.push(BlockchainRpcClient::connect(address).await?);
    }
    let mut client = submission_clients[0].clone();
    info!("✅ 成功连接到 RPC 服务器");

    // 获取初始区块信息
    let initial_block_number = client.get_block_number().await?;
    info!("📊 当前区块高度: {}", initial_block_number);

    // 初始化交易生成器
    let mut generator =
        V2TransactionGenerator::for_node(args.node_id).with_starting_nonce(args.start_nonce);
    let mut generators = (0..args.signers)
        .map(|stream_id| {
            V2TransactionGenerator::for_stream(args.node_id, stream_id as u8)
                .with_starting_nonce(args.start_nonce)
        })
        .collect::<Vec<_>>();
    if !args.faucet_peers.is_empty() {
        warn!(
            "dev_faucet mutates local state and is only safe before the first BFT finality; \
             do not use --faucet-peers during a running benchmark"
        );
        for generator in &generators {
            for faucet_peer in &args.faucet_peers {
                BlockchainRpcClient::fund_account(faucet_peer, &generator.sender()).await?;
            }
        }
        info!(
            "Funded {} bootstrap benchmark signers on {} validator RPC endpoints",
            generators.len(),
            args.faucet_peers.len()
        );
    } else {
        info!(
            "Using {} pre-funded benchmark signers from nonce {}; dev_faucet is disabled",
            generators.len(),
            args.start_nonce
        );
    }
    if args.bootstrap_only {
        info!(
            "Bootstrap funding completed for {} signer lanes; no benchmark transactions sent",
            generators.len()
        );
        return Ok(());
    }

    // 初始化统计跟踪器
    let mut stats = TestStatistics::new(args.rate, initial_block_number);

    // 计算批次间隔时间
    let transactions_per_round = if args.concurrent > 1 {
        args.batch_size.saturating_mul(args.concurrent)
    } else {
        args.batch_size
    };
    let batch_interval = Duration::from_secs_f64(transactions_per_round as f64 / args.rate as f64);
    let monitor_every_batches = (Duration::from_secs(args.monitor_interval).as_secs_f64()
        / batch_interval.as_secs_f64())
    .ceil()
    .max(1.0) as u64;

    info!("🎯 开始发送交易（目标 TPS: {}）", args.rate);
    info!("⏱️  批次间隔: {:?}", batch_interval);

    let mut presigned_offset = 0usize;
    let presigned_transactions = if args.presign {
        let target_transactions = args
            .rate
            .checked_mul(args.duration)
            .and_then(|count| usize::try_from(count).ok())
            .ok_or_else(|| anyhow::anyhow!("pre-signed workload size overflows usize"))?;
        if target_transactions > 1_000_000 {
            anyhow::bail!("--presign workload must not exceed 1,000,000 transactions");
        }
        info!(
            "Pre-signing {} transactions across {} signer lanes before timing",
            target_transactions,
            generators.len()
        );
        let started = Instant::now();
        let transactions = generate_v2_batch_parallel(&mut generators, target_transactions);
        let elapsed = started.elapsed().as_secs_f64();
        info!(
            "Pre-sign completed in {:.3} seconds ({:.2} signatures/s)",
            elapsed,
            target_transactions as f64 / elapsed.max(f64::MIN_POSITIVE)
        );
        Some(transactions)
    } else {
        None
    };

    let test_start = Instant::now();
    let test_duration = Duration::from_secs(args.duration);

    // 交易发送循环
    let mut total_sent = 0u64;
    let mut batch_count = 0u64;
    let mut accepted_transaction_ids = HashSet::new();

    while test_start.elapsed() < test_duration {
        let batch_start = Instant::now();

        // 生成并发送一批交易
        if args.concurrent <= 1 {
            for _ in 0..args.batch_size {
                let tx = generator.generate_random_transaction();

                let send_time = chrono::Utc::now().timestamp_millis();
                match client.send_transaction_v2(&tx).await {
                    Ok(tx_hash) => {
                        accepted_transaction_ids.insert(tx_hash);
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
        } else {
            let round_count = args.batch_size.saturating_mul(args.concurrent);
            let transactions = if let Some(presigned) = &presigned_transactions {
                let end = presigned_offset
                    .saturating_add(round_count)
                    .min(presigned.len());
                let batch = presigned[presigned_offset..end].to_vec();
                presigned_offset = end;
                batch
            } else {
                generate_v2_batch_parallel(&mut generators, round_count)
            };
            if transactions.is_empty() {
                break;
            }
            let send_time = chrono::Utc::now().timestamp_millis();
            let mut submissions = tokio::task::JoinSet::new();
            for (batch_index, batch) in transactions.chunks(args.batch_size).enumerate() {
                let client_index = (batch_count as usize)
                    .saturating_mul(args.concurrent)
                    .saturating_add(batch_index)
                    % submission_clients.len();
                let batch = batch.to_vec();
                if args.replicate_endpoints {
                    let clients = submission_clients.clone();
                    submissions.spawn(async move {
                        let expected = batch.len();
                        let result: anyhow::Result<Vec<String>> = async {
                            let mut replicas = tokio::task::JoinSet::new();
                            for mut client in clients {
                                let replica_batch = batch.clone();
                                replicas.spawn(async move {
                                    client.send_transactions_v2(&replica_batch).await
                                });
                            }
                            let mut canonical_hashes = None;
                            while let Some(replica) = replicas.join_next().await {
                                let hashes = replica.map_err(|error| anyhow::anyhow!(error))??;
                                canonical_hashes.get_or_insert(hashes);
                            }
                            canonical_hashes.ok_or_else(|| {
                                anyhow::anyhow!("no validator RPC endpoints configured")
                            })
                        }
                        .await;
                        (expected, result)
                    });
                } else {
                    let mut submission_client = submission_clients[client_index].clone();
                    submissions.spawn(async move {
                        let expected = batch.len();
                        (
                            expected,
                            submission_client.send_transactions_v2(&batch).await,
                        )
                    });
                }
            }
            while let Some(submission) = submissions.join_next().await {
                match submission {
                    Ok((expected, Ok(tx_hashes))) => {
                        let accepted = tx_hashes.len().min(expected);
                        accepted_transaction_ids.extend(tx_hashes.iter().take(accepted).cloned());
                        for _ in 0..accepted {
                            stats.track_submission(send_time);
                            total_sent += 1;
                        }
                        for _ in accepted..expected {
                            stats.track_failed_submission();
                        }
                        if accepted != expected {
                            error!(
                                "V2 batch response count mismatch: expected {}, got {}",
                                expected,
                                tx_hashes.len()
                            );
                        }
                    }
                    Ok((expected, Err(error))) => {
                        error!("V2 batch submission failed: {}", error);
                        for _ in 0..expected {
                            stats.track_failed_submission();
                        }
                    }
                    Err(error) => {
                        error!("V2 batch submission task failed: {}", error);
                        stats.track_failed_submission();
                    }
                }
            }
            if total_sent / 1000 != total_sent.saturating_sub(transactions.len() as u64) / 1000 {
                info!("submitted {} transactions", total_sent);
            }
        }

        batch_count += 1;

        // 定期监控进度
        if batch_count % monitor_every_batches == 0 {
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
    stats.set_submission_duration(send_duration);
    info!("✅ 交易发送完成!");
    info!("   总发送: {} 笔交易", total_sent);
    info!("   发送耗时: {:?}", send_duration);
    info!(
        "   发送速率: {:.2} TPS",
        total_sent as f64 / send_duration.as_secs_f64()
    );

    // 等待一段时间让所有交易被打包
    // A fixed post-send delay produces false zero-TPS reports when BFT rounds are
    // slower than that delay. Poll until every accepted submission is observable,
    // while retaining a bounded timeout for a genuinely stalled network.
    info!(
        "Waiting up to {} seconds for {} accepted transactions to reach canonical blocks...",
        args.settle_timeout, total_sent
    );
    let settle_start = Instant::now();
    loop {
        let observed = monitor_blockchain(
            &mut client,
            &mut stats,
            initial_block_number,
            &accepted_transaction_ids,
        )
        .await?;
        if observed >= total_sent {
            info!(
                "Observed all {} accepted transactions in canonical blocks after {:.1} seconds",
                total_sent,
                settle_start.elapsed().as_secs_f64()
            );
            break;
        }
        if settle_start.elapsed() >= Duration::from_secs(args.settle_timeout) {
            warn!(
                "Settlement timeout: observed {}/{} accepted transactions in canonical blocks",
                observed, total_sent
            );
            break;
        }
        tokio::time::sleep(Duration::from_secs(args.monitor_interval)).await;
    }

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
    accepted_transaction_ids: &HashSet<String>,
) -> Result<u64> {
    let current_block = client.get_block_number().await?;
    info!("📊 当前区块高度: {}", current_block);

    let mut total_transactions = 0u64;
    let mut transaction_blocks = 0u64;
    let mut start_timestamp: Option<i64> = None;
    let mut end_timestamp: Option<i64> = None;

    // 遍历所有新区块
    for height in (start_block + 1)..=current_block {
        match client.get_block_by_number(height).await {
            Ok(Some(block)) => {
                let tx_count = block
                    .transactions
                    .iter()
                    .filter(|transaction| {
                        accepted_transaction_ids.contains(&hex::encode(transaction.body.hash.0))
                    })
                    .count() as u64;
                total_transactions += tx_count;

                if tx_count > 0 {
                    transaction_blocks += 1;
                    let timestamp = block.header.timestamp;
                    if start_timestamp.is_none() {
                        start_timestamp = Some(timestamp);
                    }
                    end_timestamp = Some(timestamp);
                }

                if transaction_blocks > 0 && transaction_blocks % 10 == 0 {
                    info!(
                        "   已处理 {} 个含交易区块，共 {} 笔交易",
                        transaction_blocks, total_transactions
                    );
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
        // V2 block headers use Unix seconds (not the legacy millisecond timestamp).
        let time_span = (end_ts - start_ts).max(1) as f64;
        let actual_tps = if time_span > 0.0 {
            total_transactions as f64 / time_span
        } else {
            0.0
        };

        stats.set_blockchain_metrics(
            total_transactions,
            transaction_blocks,
            actual_tps,
            time_span,
        );
    } else {
        stats.set_blockchain_metrics(0, 0, 0.0, 0.0);
    }

    Ok(total_transactions)
}
