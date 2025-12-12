use num_bigint::BigInt;
use num_traits::Zero;
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, debug};
use std::ops::Rem;

// Constants
pub const RESULT_CHANNEL_CAP: usize = 32;

// Singleton
static CALCULATOR: OnceLock<Arc<Calculator>> = OnceLock::new();

pub struct Calculator {
    // Channels
    seed_tx: mpsc::Sender<BigInt>,
    prev_proof_tx: mpsc::Sender<BigInt>,

    // Internal state
    state: RwLock<CalculatorState>,

    // Immutable params
    proof_param: BigInt,
    order: BigInt,
    time_param: i64,
}

struct CalculatorState {
    prev_seed: BigInt,
    seed: BigInt,
    proof: BigInt,
    changed: bool,
}

pub fn get_calculator() -> Option<Arc<Calculator>> {
    CALCULATOR.get().cloned()
}

pub async fn init_calculator(proof_param: BigInt, order: BigInt, time_param: i64) -> Arc<Calculator> {
    let (seed_tx, seed_rx) = mpsc::channel(RESULT_CHANNEL_CAP);
    let (prev_proof_tx, prev_proof_rx) = mpsc::channel(RESULT_CHANNEL_CAP);

    let calc = Arc::new(Calculator {
        seed_tx,
        prev_proof_tx,
        state: RwLock::new(CalculatorState {
            prev_seed: BigInt::zero(),
            seed: BigInt::zero(),
            proof: BigInt::zero(),
            changed: false,
        }),
        proof_param,
        order,
        time_param,
    });

    // Spawn run loop
    let c = calc.clone();
    tokio::spawn(async move {
        c.run_loop(seed_rx, prev_proof_rx).await;
    });

    // Only set if not already set
    let _ = CALCULATOR.set(calc.clone());
    calc
}

impl Calculator {
    // Port: GetSeedParams (Async)
    pub async fn get_seed_params(&self) -> (BigInt, BigInt) {
        let state = self.state.read().await;
        (state.seed.clone(), state.proof.clone())
    }

    // Port: VerifyBlockVDF
    pub async fn verify_block_vdf(&self, seed: &BigInt, proof: &BigInt) -> bool {
        let state = self.state.read().await;
        
        if state.prev_seed == *seed || state.seed == *seed {
            return true;
        }
        
        if !state.seed.is_zero() && self.verify(&state.seed, proof, seed) {
            return true;
        }
        
        false
    }

    // Port: AppendNewSeed
    pub async fn append_new_seed(&self, seed: &BigInt, proof: &BigInt) {
        let mut state = self.state.write().await;
        debug!("Current VDF seed: {}", state.seed);
        
        if state.prev_seed == *seed || state.seed == *seed {
            return;
        }
        
        if !state.seed.is_zero() && !self.verify(&state.seed, proof, seed) {
            debug!("Block VDF verify failed");
            return;
        }
        
        state.changed = true;
        debug!("New Seed: {}, Proof: {}", seed, proof);
        
        // Note: We use try_send or send. If channel full, it might delay.
        let _ = self.seed_tx.send(seed.clone()).await;
        let _ = self.prev_proof_tx.send(proof.clone()).await;
    }

    // Port: Verify (Simple VDF)
    pub fn verify(&self, seed: &BigInt, pi: &BigInt, result: &BigInt) -> bool {
        // r = 2
        let mut r = BigInt::from(2);
        let t = BigInt::from(self.time_param);
        
        // r = r^t mod pp
        r = r.modpow(&t, &self.proof_param);
        
        // h = pi^pp mod order
        let mut h = pi.modpow(&self.proof_param, &self.order);
        
        // s = seed^r mod order
        let s = seed.modpow(&r, &self.order);
        
        h = (&h * &s).rem(&self.order);
        
        result == &h
    }
    
    // Port: run loop
    async fn run_loop(&self, mut seed_rx: mpsc::Receiver<BigInt>, mut prev_proof_rx: mpsc::Receiver<BigInt>) {
        while let Some(seed) = seed_rx.recv().await {
            info!("Starting VDF calculation with seed: {}", seed);

            let proof = match prev_proof_rx.recv().await {
                Some(p) => p,
                None => break,
            };

            {
                let mut state = self.state.write().await;
                state.changed = false;
                state.prev_seed = state.seed.clone();
                state.seed = seed.clone();
                state.proof = proof.clone();
                info!("Set seed to {}", state.seed);
            }

            // Perform VDF calculation (simplified)
            let result = self.calculate_vdf(&seed, &proof).await;
            info!("VDF calculation result: {}", result);

            // Update state with calculation result
            {
                let mut state = self.state.write().await;
                state.proof = result;
            }
        }
    }

    // Actual VDF calculation
    async fn calculate_vdf(&self, seed: &BigInt, proof: &BigInt) -> BigInt {
        // Simulate time-based VDF calculation
        let iterations = self.time_param as usize;

        let mut result = seed.clone();

        // Simple sequential squaring to simulate work
        for i in 0..iterations {
            if i % 100000 == 0 {
                // Yield control periodically to avoid blocking
                tokio::task::yield_now().await;
            }
            result = result.modpow(&BigInt::from(2), &self.order);
        }

        // Apply proof
        (&result * proof).rem(&self.order)
    }
}
