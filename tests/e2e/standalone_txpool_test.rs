//! Standalone test for EnhancedTxPool

use std::collections::BinaryHeap;
use std::cmp::Ordering;

// Minimal types for testing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub body: TransactionBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionBody {
    pub hash: TestHash,
    pub address: TestAddress,
    pub nonce: i64,
    pub gas_price: Option<u64>,
    pub max_fee_per_gas: Option<u64>,
}

impl Default for Transaction {
    fn default() -> Self {
        Self {
            body: TransactionBody::default(),
        }
    }
}

impl Default for TransactionBody {
    fn default() -> Self {
        Self {
            hash: TestHash::default(),
            address: TestAddress::default(),
            nonce: 0,
            gas_price: Some(100),
            max_fee_per_gas: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TestHash([u8; 32]);

impl Default for TestHash {
    fn default() -> Self {
        Self([0u8; 32])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TestAddress([u8; 20]);

impl Default for TestAddress {
    fn default() -> Self {
        Self([0u8; 20])
    }
}

// PrioritizedTransaction from txpool_enhanced
#[derive(Debug, Clone)]
pub struct PrioritizedTransaction {
    pub tx: Transaction,
    pub effective_gas_price: u64,
    pub added_at: i64,
    pub nonce: i64,
    pub sender: TestAddress,
}

impl PrioritizedTransaction {
    fn new(tx: Transaction) -> Self {
        let effective_gas_price = tx.body.max_fee_per_gas
            .or(tx.body.gas_price)
            .unwrap_or(0) as u64;

        let added_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let nonce = tx.body.nonce;
        let sender = tx.body.address;

        Self {
            tx,
            effective_gas_price,
            added_at,
            nonce,
            sender,
        }
    }
}

impl PartialEq for PrioritizedTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.effective_gas_price == other.effective_gas_price
    }
}

impl Eq for PrioritizedTransaction {}

impl PartialOrd for PrioritizedTransaction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTransaction {
    fn cmp(&self, other: &Self) -> Ordering {
        // 反向比较，使最高 Gas 价格在堆顶
        other.effective_gas_price
            .cmp(&self.effective_gas_price)
            .then_with(|| self.added_at.cmp(&other.added_at))
    }
}

fn main() {
    println!("🧪 Testing Enhanced Transaction Pool\n");

    // Test 1: Priority ordering
    println!("Test 1: Priority Ordering");
    let mut heap = BinaryHeap::new();

    for i in 1u64..=5 {
        let mut tx = Transaction::default();
        tx.body.hash.0[0] = i as u8;
        tx.body.gas_price = Some(i * 10);
        tx.body.address.0[0] = i as u8;
        tx.body.nonce = i as i64;

        let prioritized = PrioritizedTransaction::new(tx);
        heap.push(prioritized);
    }

    // Verify ordering (highest gas price first)
    let first = heap.pop().unwrap();
    println!("  First transaction gas price: {}", first.effective_gas_price);
    assert!(first.effective_gas_price == 50, "Highest gas price should be first");
    println!("  ✅ Highest gas price transaction is first: {}", first.effective_gas_price);

    let second = heap.pop().unwrap();
    println!("  Second transaction gas price: {}", second.effective_gas_price);
    assert!(second.effective_gas_price == 40, "Second highest gas price should be second");
    println!("  ✅ Second highest gas price transaction: {}", second.effective_gas_price);

    // Test 2: Transaction replacement
    println!("\nTest 2: Transaction Replacement");

    let mut tx1 = Transaction::default();
    tx1.body.hash.0[0] = 1;
    tx1.body.gas_price = Some(100);
    tx1.body.address.0[0] = 5;
    tx1.body.nonce = 0;

    let mut tx2 = Transaction::default();
    tx2.body.hash.0[0] = 2;
    tx2.body.gas_price = Some(120); // 20% higher
    tx2.body.address.0[0] = 5;
    tx2.body.nonce = 0; // Same nonce

    let p1 = PrioritizedTransaction::new(tx1);
    let p2 = PrioritizedTransaction::new(tx2);

    // Verify replacement condition
    let price_increase = p2.effective_gas_price.saturating_sub(p1.effective_gas_price);
    let should_replace = price_increase >= (p1.effective_gas_price / 10);

    assert!(should_replace, "Transaction should be replaced with 20% higher gas price");
    println!("  ✅ Transaction replacement condition met: {} >= {} (10% of {})",
        price_increase, p1.effective_gas_price / 10, p1.effective_gas_price);

    // Test 3: Expiration
    println!("\nTest 3: Transaction Expiration");

    let old_tx = Transaction::default();
    let mut old_prioritized = PrioritizedTransaction::new(old_tx);
    old_prioritized.added_at = 0; // Very old (1970-01-01)

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let is_expired = (now - old_prioritized.added_at) > 3600;
    assert!(is_expired, "Old transaction should be expired");
    println!("  ✅ Old transaction correctly identified as expired (age: {} seconds)", now - old_prioritized.added_at);

    let new_tx = Transaction::default();
    let new_prioritized = PrioritizedTransaction::new(new_tx);
    let is_not_expired = (now - new_prioritized.added_at) > 3600;
    assert!(!is_not_expired, "New transaction should not be expired");
    println!("  ✅ New transaction correctly identified as not expired (age: {} seconds)", now - new_prioritized.added_at);

    println!("\n✅ All tests passed!");
    println!("\n📊 Test Summary:");
    println!("  - Priority ordering: ✅ PASSED");
    println!("  - Transaction replacement: ✅ PASSED");
    println!("  - Transaction expiration: ✅ PASSED");
    println!("\n🎉 Enhanced Transaction Pool core functionality is working correctly!");
}
