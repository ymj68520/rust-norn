//! Protocol-V2 microbenchmarks intended to run on the target architecture.
//! They report per-operation distributions rather than one aggregate wall time.

use std::hint::black_box;
use std::time::Instant;

use anyhow::{bail, Result};
use norn_common::types::{
    AccessListItem, Address, ChainId, Hash, ProtocolVersion, TransactionId, TransactionType,
    TransactionV2,
};
use norn_core::txpool_v2::TransactionV2Pool;
use norn_crypto::ecdsa::KeyPair;
use norn_crypto::transaction::{
    sign_transaction_v2, verify_transaction_v2, verify_transaction_v2_uncached,
};
use sha2::{Digest, Sha256};

const DEFAULT_ITERATIONS: usize = 10_000;
const SELECT_ITERATIONS: usize = 1_000;

fn usage() -> &'static str {
    "Usage: crypto_bench [--iterations N]\n\
     Measures protocol-V2 signing, verification, serialization, pool admission,\n\
     and deterministic pool selection on the current CPU."
}

fn iterations_from_args() -> Result<usize> {
    let mut args = std::env::args().skip(1);
    let mut iterations = DEFAULT_ITERATIONS;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--iterations" => {
                let Some(value) = args.next() else {
                    bail!("--iterations requires a positive integer");
                };
                iterations = value.parse()?;
            }
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            _ => bail!("unknown argument {arg}\n{}", usage()),
        }
    }
    if iterations == 0 {
        bail!("--iterations must be greater than zero");
    }
    Ok(iterations)
}

fn benchmark<F>(name: &str, iterations: usize, mut operation: F) -> Result<()>
where
    F: FnMut(usize) -> Result<()>,
{
    // Warm up CPU frequency scaling, allocation paths, and crypto precomputation.
    for index in 0..iterations.min(100) {
        operation(index)?;
    }

    let mut samples = Vec::with_capacity(iterations);
    for index in 0..iterations {
        let started = Instant::now();
        operation(index)?;
        samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }
    samples.sort_by(|left, right| left.total_cmp(right));
    let mean_us = samples.iter().sum::<f64>() / samples.len() as f64;
    let median_us = samples[samples.len() / 2];
    let p95_index = ((samples.len() - 1) as f64 * 0.95).ceil() as usize;
    let p95_us = samples[p95_index];
    let operations_per_second = 1_000_000.0 / mean_us.max(f64::MIN_POSITIVE);
    println!(
        "METRIC,{name},{iterations},{median_us:.3},{p95_us:.3},{mean_us:.3},{operations_per_second:.2}"
    );
    Ok(())
}

fn address_for(keypair: &KeyPair) -> Address {
    let encoded = keypair.public_key().to_encoded_point(true);
    let digest = Sha256::digest(encoded.as_bytes());
    Address(
        digest[..20]
            .try_into()
            .expect("SHA-256 prefix has 20 bytes"),
    )
}

fn unsigned_transaction(sender: Address, nonce: u64) -> TransactionV2 {
    TransactionV2 {
        protocol_version: ProtocolVersion(5),
        chain_id: ChainId(Hash([1; 32])),
        nonce,
        sender,
        receiver: Some(Address([0x42; 20])),
        value: 0,
        gas_limit: 21_000,
        max_fee_per_gas: 1,
        max_priority_fee_per_gas: 0,
        data: Vec::new(),
        event: Vec::new(),
        opt: Vec::new(),
        state: Vec::new(),
        expire: None,
        timestamp: 1_723_456_789,
        tx_type: TransactionType::Native,
        access_list: Vec::<AccessListItem>::new(),
        public_key: Default::default(),
        signature: [0; 64],
        transaction_id: TransactionId::default(),
    }
}

fn signed_transaction(keypair: &KeyPair, sender: Address, nonce: u64) -> Result<TransactionV2> {
    let mut transaction = unsigned_transaction(sender, nonce);
    sign_transaction_v2(keypair, &mut transaction)?;
    Ok(transaction)
}

fn main() -> Result<()> {
    let iterations = iterations_from_args()?;
    let keypair = KeyPair::from_private_key_hex(&"11".repeat(32))?;
    let sender = address_for(&keypair);
    let signed = signed_transaction(&keypair, sender, 0)?;
    verify_transaction_v2(&signed)?;

    println!("# crypto_bench,v2,iterations={iterations}");
    println!("# columns,name,iterations,median_us,p95_us,mean_us,ops_per_sec");

    benchmark("v2_sign", iterations, |index| {
        let mut transaction = unsigned_transaction(sender, index as u64);
        sign_transaction_v2(&keypair, &mut transaction)?;
        black_box(transaction);
        Ok(())
    })?;
    benchmark("v2_verify_uncached", iterations, |_| {
        verify_transaction_v2_uncached(black_box(&signed))?;
        Ok(())
    })?;
    benchmark("v2_verify_cached", iterations, |_| {
        verify_transaction_v2(black_box(&signed))?;
        Ok(())
    })?;
    benchmark("v2_bincode_encode", iterations, |_| {
        black_box(bincode::serialize(&signed)?);
        Ok(())
    })?;

    let pool_transactions = (0..iterations + 100)
        .map(|index| signed_transaction(&keypair, sender, index as u64))
        .collect::<Result<Vec<_>>>()?;
    let admission_pool = TransactionV2Pool::new_with_capacity(pool_transactions.len() + 1);
    benchmark("v2_pool_admit", iterations, |index| {
        admission_pool.add(pool_transactions[index].clone())?;
        Ok(())
    })?;

    let select_pool = TransactionV2Pool::new_with_capacity(1_024);
    for transaction in pool_transactions.iter().take(1_024) {
        select_pool.add(transaction.clone())?;
    }
    benchmark(
        "v2_pool_select_64_from_1024",
        SELECT_ITERATIONS.min(iterations),
        |_| {
            black_box(select_pool.select(64));
            Ok(())
        },
    )?;
    Ok(())
}
