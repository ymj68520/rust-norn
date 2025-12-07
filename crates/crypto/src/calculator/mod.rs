use num_bigint::{BigInt, RandBigInt};
use num_traits::{One, Zero, Num};
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, debug, error};
use std::ops::{Mul, Div, Rem};

// Constants
pub const RESULT_CHANNEL_CAP: usize = 32;

// Singleton
static CALCULATOR: OnceLock<Arc<Calculator>> = OnceLock::new();

pub struct Calculator {
    // Channels
    seed_tx: mpsc::Sender<BigInt>,
    prev_proof_tx: mpsc::Sender<BigInt>,
    result_tx: mpsc::Sender<BigInt>,
    proof_tx: mpsc::Sender<BigInt>,
    
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
    let (result_tx, _result_rx) = mpsc::channel(RESULT_CHANNEL_CAP); // _result_rx for consumption by GetSeedParams logic
    let (proof_tx, _proof_rx) = mpsc::channel(RESULT_CHANNEL_CAP);

    let calc = Arc::new(Calculator {
        seed_tx,
        prev_proof_tx,
        result_tx,
        proof_tx,
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
            info!("Start new VDF calculate.");
            
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
            
            // Stub for actual calculation loop
        }
    }
}
