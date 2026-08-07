use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 交易跟踪器
#[derive(Debug)]
pub struct TransactionTracker {
    pub submission_time: i64,
    pub included: bool,
}

/// 测试统计数据
pub struct TestStatistics {
    /// 目标 TPS
    target_tps: u64,

    /// 起始区块高度
    start_block: i64,

    /// 已提交的交易数
    submitted_transactions: Arc<AtomicU64>,

    /// 失败的交易数
    failed_transactions: Arc<AtomicU64>,

    /// 实际打包的交易数
    packed_transactions: Arc<AtomicU64>,

    /// 区块链实际 TPS，以毫 TPS 保存，避免丢失小数精度。
    actual_tps_milli: Arc<AtomicU64>,

    /// 首尾含交易区块之间的提交窗口，单位为毫秒。
    blockchain_span_millis: Arc<AtomicU64>,

    /// 总区块数
    total_blocks: Arc<AtomicU64>,

    // Submission phase only; finality-settlement polling must not dilute
    // injection TPS.
    submission_span_millis: Arc<AtomicU64>,

    /// 测试开始时间
    test_start: Option<Instant>,

    /// 是否已完成
    _completed: Arc<AtomicBool>,
}

impl TestStatistics {
    /// 创建新的统计实例
    pub fn new(target_tps: u64, start_block: i64) -> Self {
        Self {
            target_tps,
            start_block,
            submitted_transactions: Arc::new(AtomicU64::new(0)),
            failed_transactions: Arc::new(AtomicU64::new(0)),
            packed_transactions: Arc::new(AtomicU64::new(0)),
            actual_tps_milli: Arc::new(AtomicU64::new(0)),
            blockchain_span_millis: Arc::new(AtomicU64::new(0)),
            total_blocks: Arc::new(AtomicU64::new(0)),
            submission_span_millis: Arc::new(AtomicU64::new(0)),
            test_start: Some(Instant::now()),
            _completed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 跟踪交易提交
    pub fn track_submission(&self, _timestamp: i64) {
        self.submitted_transactions.fetch_add(1, Ordering::Relaxed);
    }

    /// 跟踪失败的交易提交
    pub fn track_failed_submission(&self) {
        self.failed_transactions.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the elapsed time spent submitting the measured stream before
    /// the later canonical-settlement phase begins.
    pub fn set_submission_duration(&self, duration: Duration) {
        self.submission_span_millis.store(
            duration.as_millis().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    /// Submission throughput, independent of any consensus-settlement wait.
    pub fn submission_tps(&self) -> f64 {
        let millis = self.submission_span_millis.load(Ordering::Relaxed);
        if millis == 0 {
            0.0
        } else {
            self.submitted() as f64 * 1_000.0 / millis as f64
        }
    }

    /// 设置区块链指标
    pub fn set_blockchain_metrics(
        &self,
        total_tx: u64,
        total_blocks: u64,
        actual_tps: f64,
        time_span: f64,
    ) {
        self.packed_transactions.store(total_tx, Ordering::Relaxed);
        self.total_blocks.store(total_blocks, Ordering::Relaxed);
        self.actual_tps_milli.store(
            (actual_tps.max(0.0) * 1_000.0).round() as u64,
            Ordering::Relaxed,
        );
        self.blockchain_span_millis.store(
            (time_span.max(0.0) * 1_000.0).round() as u64,
            Ordering::Relaxed,
        );
    }

    /// 获取已提交交易数
    pub fn submitted(&self) -> u64 {
        self.submitted_transactions.load(Ordering::Relaxed)
    }

    /// 获取失败交易数
    pub fn failed(&self) -> u64 {
        self.failed_transactions.load(Ordering::Relaxed)
    }

    /// 获取打包交易数
    pub fn packed(&self) -> u64 {
        self.packed_transactions.load(Ordering::Relaxed)
    }

    /// 获取实际 TPS
    pub fn actual_tps(&self) -> f64 {
        self.actual_tps_milli.load(Ordering::Relaxed) as f64 / 1_000.0
    }

    /// 获取首尾含交易区块的时间跨度，单位为秒。
    pub fn blockchain_time_span(&self) -> f64 {
        self.blockchain_span_millis.load(Ordering::Relaxed) as f64 / 1_000.0
    }

    /// 获取区块数
    pub fn total_blocks(&self) -> u64 {
        self.total_blocks.load(Ordering::Relaxed)
    }

    /// 计算成功率
    pub fn success_rate(&self) -> f64 {
        let submitted = self.submitted() as f64;
        let failed = self.failed() as f64;

        if submitted + failed > 0.0 {
            (submitted / (submitted + failed)) * 100.0
        } else {
            0.0
        }
    }

    /// 计算达成率（实际 TPS / 目标 TPS）
    pub fn achievement_rate(&self) -> f64 {
        if self.target_tps > 0 {
            (self.actual_tps() / self.target_tps as f64) * 100.0
        } else {
            0.0
        }
    }

    /// 计算平均每块交易数
    pub fn avg_tx_per_block(&self) -> f64 {
        let blocks = self.total_blocks() as f64;
        let packed = self.packed() as f64;

        if blocks > 0.0 {
            packed / blocks
        } else {
            0.0
        }
    }

    /// 打印统计报告
    pub fn print_report(&self) {
        let elapsed = self
            .test_start
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);

        println!("📊 测试配置:");
        println!("   ├─ 目标 TPS: {}", self.target_tps);
        println!("   ├─ 起始区块: {}", self.start_block);
        println!("   └─ 测试时长: {:.2} 秒", elapsed.as_secs_f64());

        println!("\n📦 交易提交统计:");
        println!("   ├─ 已提交: {} 笔", self.submitted());
        println!("   ├─ 失败: {} 笔", self.failed());
        println!("   ├─ 成功率: {:.2}%", self.success_rate());
        println!("   └─ 提交速率: {:.2} TPS", self.submission_tps());

        println!("\n⛓️  区块链打包统计:");
        println!("   ├─ 打包交易: {} 笔", self.packed());
        println!("   ├─ 含交易区块: {} 个", self.total_blocks());
        println!("   ├─ 打包窗口: {:.2} 秒", self.blockchain_time_span());
        println!("   ├─ 实际 TPS: {:.2}", self.actual_tps());
        println!("   ├─ 达成率: {:.2}%", self.achievement_rate());
        println!("   └─ 平均每块交易: {:.2}", self.avg_tx_per_block());

        println!("\n📈 性能分析:");
        let achievement = self.achievement_rate();
        if achievement >= 90.0 {
            println!("   ✅ 优秀: TPS 达成率 {:.2}% >= 90%", achievement);
        } else if achievement >= 70.0 {
            println!("   ⚠️  良好: TPS 达成率 {:.2}% >= 70%", achievement);
        } else if achievement >= 50.0 {
            println!("   ⚠️  一般: TPS 达成率 {:.2}% >= 50%", achievement);
        } else {
            println!("   ❌ 需要优化: TPS 达成率 {:.2}% < 50%", achievement);
        }

        let success_rate = self.success_rate();
        if success_rate >= 99.0 {
            println!("   ✅ 优秀: 交易成功率 {:.2}% >= 99%", success_rate);
        } else if success_rate >= 95.0 {
            println!("   ⚠️  良好: 交易成功率 {:.2}% >= 95%", success_rate);
        } else {
            println!("   ❌ 需要优化: 交易成功率 {:.2}% < 95%", success_rate);
        }

        // 打包率分析
        if self.submitted() > 0 {
            let packing_rate = (self.packed() as f64 / self.submitted() as f64) * 100.0;
            println!("   📊 交易打包率: {:.2}%", packing_rate);
        }
    }

    /// 生成 CSV 格式的报告
    pub fn to_csv(&self) -> String {
        let elapsed = self
            .test_start
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);

        format!(
            "{},{},{},{},{},{},{},{},{:.2},{:.2},{:.2}\n",
            self.target_tps,
            self.submitted(),
            self.failed(),
            self.packed(),
            self.total_blocks(),
            self.actual_tps(),
            elapsed.as_secs_f64(),
            self.success_rate(),
            self.achievement_rate(),
            self.avg_tx_per_block(),
            self.submission_tps()
        )
    }

    /// 生成 CSV 表头
    pub fn csv_header() -> String {
        "target_tps,submitted,failed,packed,total_blocks,actual_tps,duration_sec,success_rate%,achievement_rate%,avg_tx_per_block,submit_tps\n".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statistics_creation() {
        let stats = TestStatistics::new(100, 0);
        assert_eq!(stats.submitted(), 0);
        assert_eq!(stats.failed(), 0);
        assert_eq!(stats.target_tps, 100);
    }

    #[test]
    fn test_track_submission() {
        let stats = TestStatistics::new(100, 0);
        stats.track_submission(chrono::Utc::now().timestamp_millis());
        assert_eq!(stats.submitted(), 1);
    }

    #[test]
    fn test_track_failed() {
        let stats = TestStatistics::new(100, 0);
        stats.track_failed_submission();
        assert_eq!(stats.failed(), 1);
    }

    #[test]
    fn submission_tps_excludes_finality_settlement_time() {
        let stats = TestStatistics::new(100, 0);
        stats.track_submission(0);
        stats.track_submission(0);
        stats.set_submission_duration(Duration::from_millis(500));

        assert!((stats.submission_tps() - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate() {
        let stats = TestStatistics::new(100, 0);
        stats.track_submission(chrono::Utc::now().timestamp_millis());
        stats.track_submission(chrono::Utc::now().timestamp_millis());
        stats.track_failed_submission();
        assert!((stats.success_rate() - 66.66).abs() < 0.1);
    }

    #[test]
    fn test_achievement_rate() {
        let stats = TestStatistics::new(100, 0);
        stats.set_blockchain_metrics(80, 10, 80.0, 1.0);
        assert!((stats.achievement_rate() - 80.0).abs() < 0.1);
    }
}
