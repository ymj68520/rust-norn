#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::Block;
    
    // We can add integration test for Calculator flow
    // Since calculate is stubbed, we test flow logic (channels/state update)
    
    #[tokio::test]
    async fn test_calculator_state_update() {
        let proof_param = BigInt::from(100);
        let order = BigInt::from(200);
        let time_param = 10;
        
        let calc = init_calculator(proof_param, order, time_param).await;
        
        let seed = BigInt::from(12345);
        let proof = BigInt::from(67890);
        
        calc.append_new_seed(&seed, &proof).await;
        
        // Wait a bit for async update
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        
        let (s, p) = calc.get_seed_params().await;
        assert_eq!(s, seed);
        assert_eq!(p, proof);
    }
}
