use std::collections::VecDeque;
/// Rate-Limiting Utilities Example
///
/// This example demonstrates various rate-limiting strategies for blockchain RPC calls:
///
/// Rate-Limiting Strategies:
/// 1. Token Bucket: Refill tokens at a fixed rate
/// 2. Sliding Window: Track requests in a time window
/// 3. Adaptive Rate-Limit: Adjust rate based on response codes
/// 4. Per-Method Limits: Different limits for different RPC methods
/// 5. Backoff Strategy: Exponential backoff on rate-limit errors
///
/// Why rate-limiting is important:
/// - Most RPC providers have rate limits
/// - Prevents overwhelming the node
/// - Protects against accidental DoS
/// - Manages costs on paid RPC services
/// - Improves reliability and consistency
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Token bucket rate limiter
/// Allows N requests per duration, with optional burst capacity
#[derive(Clone)]
pub struct TokenBucket {
    capacity: usize,
    tokens: Arc<Mutex<usize>>,
    refill_rate: f64, // tokens per second
    last_refill: Arc<Mutex<Instant>>,
}

impl TokenBucket {
    /// Create a new token bucket
    /// capacity: max tokens (also burst capacity)
    /// refill_rate: tokens per second
    pub fn new(capacity: usize, refill_rate: f64) -> Self {
        TokenBucket {
            capacity,
            tokens: Arc::new(Mutex::new(capacity)),
            refill_rate,
            last_refill: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Try to take a token, returns true if successful
    pub fn try_acquire(&self) -> bool {
        let mut tokens = self.tokens.lock().unwrap();
        let mut last_refill = self.last_refill.lock().unwrap();

        // Calculate tokens to add based on time passed
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill).as_secs_f64();
        let tokens_to_add = (elapsed * self.refill_rate) as usize;

        if tokens_to_add > 0 {
            *tokens = (*tokens + tokens_to_add).min(self.capacity);
            *last_refill = now;
        }

        if *tokens > 0 {
            *tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Get time to wait before next token is available (in milliseconds)
    pub fn time_until_available_ms(&self) -> u64 {
        let tokens = self.tokens.lock().unwrap();
        let last_refill = self.last_refill.lock().unwrap();

        if *tokens > 0 {
            return 0;
        }

        let time_to_next_token = 1.0 / self.refill_rate;
        (time_to_next_token * 1000.0) as u64
    }
}

/// Sliding window rate limiter
/// Tracks requests in a time window
pub struct SlidingWindow {
    max_requests: usize,
    window_duration: Duration,
    requests: Arc<Mutex<VecDeque<Instant>>>,
}

impl SlidingWindow {
    /// Create a new sliding window limiter
    /// max_requests: max requests in the window
    /// window_duration: time window duration
    pub fn new(max_requests: usize, window_duration: Duration) -> Self {
        SlidingWindow {
            max_requests,
            window_duration,
            requests: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Try to make a request
    pub fn try_acquire(&self) -> bool {
        let mut requests = self.requests.lock().unwrap();
        let now = Instant::now();

        // Remove old requests outside the window
        while let Some(&first_time) = requests.front() {
            if now.duration_since(first_time) > self.window_duration {
                requests.pop_front();
            } else {
                break;
            }
        }

        // Check if we can add a new request
        if requests.len() < self.max_requests {
            requests.push_back(now);
            true
        } else {
            false
        }
    }

    /// Get current request count in the window
    pub fn current_requests(&self) -> usize {
        let mut requests = self.requests.lock().unwrap();
        let now = Instant::now();

        // Clean up old requests
        while let Some(&first_time) = requests.front() {
            if now.duration_since(first_time) > self.window_duration {
                requests.pop_front();
            } else {
                break;
            }
        }

        requests.len()
    }
}

/// Per-method rate limiter
/// Different limits for different RPC methods
pub struct PerMethodLimiter {
    limiters: Arc<Mutex<std::collections::HashMap<String, TokenBucket>>>,
}

impl PerMethodLimiter {
    /// Create a new per-method limiter
    pub fn new() -> Self {
        PerMethodLimiter {
            limiters: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Register a method with rate limit
    /// method: RPC method name
    /// capacity: token bucket capacity
    /// refill_rate: tokens per second
    pub fn register_method(&self, method: &str, capacity: usize, refill_rate: f64) {
        let mut limiters = self.limiters.lock().unwrap();
        limiters.insert(method.to_string(), TokenBucket::new(capacity, refill_rate));
    }

    /// Try to acquire for a method
    pub fn try_acquire(&self, method: &str) -> bool {
        let limiters = self.limiters.lock().unwrap();

        if let Some(limiter) = limiters.get(method) {
            limiter.try_acquire()
        } else {
            // Method not registered, allow it
            true
        }
    }
}

/// Exponential backoff helper
pub struct ExponentialBackoff {
    initial_delay_ms: u64,
    max_delay_ms: u64,
    multiplier: f64,
    attempt: usize,
}

impl ExponentialBackoff {
    /// Create a new exponential backoff
    pub fn new(initial_delay_ms: u64, max_delay_ms: u64) -> Self {
        ExponentialBackoff {
            initial_delay_ms,
            max_delay_ms,
            multiplier: 2.0,
            attempt: 0,
        }
    }

    /// Get delay for current attempt (in milliseconds)
    pub fn get_delay_ms(&self) -> u64 {
        let delay =
            (self.initial_delay_ms as f64 * self.multiplier.powi(self.attempt as i32)) as u64;
        delay.min(self.max_delay_ms)
    }

    /// Increment attempt counter
    pub fn next_attempt(&mut self) {
        self.attempt += 1;
    }

    /// Reset attempt counter
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Check if max retries reached (arbitrary limit of 10 attempts)
    pub fn should_retry(&self) -> bool {
        self.attempt < 10
    }
}

/// Adaptive rate limiter
/// Adjusts rate based on error responses
pub struct AdaptiveRateLimiter {
    current_rate: Arc<Mutex<f64>>, // requests per second
    min_rate: f64,
    max_rate: f64,
    decrease_factor: f64,
    increase_factor: f64,
}

impl AdaptiveRateLimiter {
    /// Create a new adaptive rate limiter
    pub fn new(initial_rate: f64, min_rate: f64, max_rate: f64) -> Self {
        AdaptiveRateLimiter {
            current_rate: Arc::new(Mutex::new(initial_rate)),
            min_rate,
            max_rate,
            decrease_factor: 0.5, // Halve on error
            increase_factor: 1.1, // Increase 10% on success
        }
    }

    /// Record a successful request
    pub fn record_success(&self) {
        let mut rate = self.current_rate.lock().unwrap();
        *rate = (*rate * self.increase_factor).min(self.max_rate);
    }

    /// Record a failed request (rate limit hit)
    pub fn record_rate_limit_error(&self) {
        let mut rate = self.current_rate.lock().unwrap();
        *rate = (*rate * self.decrease_factor).max(self.min_rate);
    }

    /// Get current rate
    pub fn get_current_rate(&self) -> f64 {
        *self.current_rate.lock().unwrap()
    }

    /// Get token bucket based on current rate
    pub fn get_token_bucket(&self) -> TokenBucket {
        let rate = self.get_current_rate();
        let capacity = (rate.ceil() as usize).max(1);
        TokenBucket::new(capacity, rate)
    }
}

/// Main example demonstrating rate-limiting strategies
fn main() {
    println!("=== Rate-Limiting Utilities Examples ===\n");

    // Example 1: Token Bucket Rate Limiter
    println!("1. Token Bucket Rate Limiter:");
    println!("   Allows 10 requests per second with burst capacity of 20");
    let tb = TokenBucket::new(20, 10.0);

    for i in 1..=25 {
        if tb.try_acquire() {
            println!("   Request {} - ALLOWED", i);
        } else {
            let wait_ms = tb.time_until_available_ms();
            println!("   Request {} - DENIED (wait {}ms)", i, wait_ms);
        }
    }

    // Example 2: Sliding Window Rate Limiter
    println!("\n2. Sliding Window Rate Limiter:");
    println!("   Max 5 requests per 1 second window");
    let sw = SlidingWindow::new(5, Duration::from_secs(1));

    for i in 1..=7 {
        if sw.try_acquire() {
            println!(
                "   Request {} - ALLOWED (total in window: {})",
                i,
                sw.current_requests()
            );
        } else {
            println!("   Request {} - DENIED (window full)", i);
        }
    }

    // Example 3: Per-Method Rate Limiting
    println!("\n3. Per-Method Rate Limiting:");
    println!("   Different limits for different RPC methods");
    let pm = PerMethodLimiter::new();

    // Register methods with different rates
    pm.register_method("eth_call", 100, 50.0); // 50 per second, burst 100
    pm.register_method("eth_getBalance", 50, 20.0); // 20 per second, burst 50
    pm.register_method("eth_sendRawTransaction", 10, 5.0); // 5 per second, burst 10

    println!(
        "   eth_call: {}",
        if pm.try_acquire("eth_call") {
            "✓ ALLOWED"
        } else {
            "✗ DENIED"
        }
    );
    println!(
        "   eth_getBalance: {}",
        if pm.try_acquire("eth_getBalance") {
            "✓ ALLOWED"
        } else {
            "✗ DENIED"
        }
    );
    println!(
        "   eth_sendRawTransaction: {}",
        if pm.try_acquire("eth_sendRawTransaction") {
            "✓ ALLOWED"
        } else {
            "✗ DENIED"
        }
    );

    // Example 4: Exponential Backoff
    println!("\n4. Exponential Backoff Strategy:");
    println!("   Backoff delays for retries after rate limit");
    let mut backoff = ExponentialBackoff::new(100, 30000);

    for attempt in 0..5 {
        println!(
            "   Attempt {} - Wait {}ms",
            attempt + 1,
            backoff.get_delay_ms()
        );
        backoff.next_attempt();
    }

    // Example 5: Adaptive Rate Limiting
    println!("\n5. Adaptive Rate Limiting:");
    println!("   Adjusts rate based on error responses");
    let adaptive = AdaptiveRateLimiter::new(50.0, 5.0, 100.0);

    println!(
        "   Initial rate: {:.1} req/sec",
        adaptive.get_current_rate()
    );

    // Simulate successes
    for _ in 0..3 {
        adaptive.record_success();
    }
    println!(
        "   After 3 successes: {:.1} req/sec",
        adaptive.get_current_rate()
    );

    // Simulate rate limit error
    adaptive.record_rate_limit_error();
    println!(
        "   After rate limit error: {:.1} req/sec",
        adaptive.get_current_rate()
    );

    // Example 6: Common rate limit patterns for popular providers
    println!("\n6. Rate Limit Patterns for Popular Providers:");
    println!("   Infura: 100 requests/sec");
    println!("   Alchemy: 300 requests/second (up to 3,000 CU/sec)");
    println!("   QuickNode: 100-200 requests/sec depending on plan");
    println!("   Etherscan: 5 calls/second free tier");

    // Example 7: Best practices
    println!("\n=== Best Practices ===");
    println!("✓ Always implement rate limiting for production applications");
    println!("✓ Use token bucket for predictable, consistent rates");
    println!("✓ Use sliding window for strict per-second limits");
    println!("✓ Implement exponential backoff for retries");
    println!("✓ Monitor 429 (Too Many Requests) responses");
    println!("✓ Consider using adaptive limits for dynamic environments");
    println!("✓ Batch requests when possible to reduce total calls");
    println!("✓ Cache read-only data to avoid repeated queries");

    println!("\n=== Implementation Tips ===");
    println!("1. Start with token bucket (most flexible)");
    println!("2. Combine with per-method limits for fine-grained control");
    println!("3. Add exponential backoff for resilience");
    println!("4. Monitor actual rate and adjust limits as needed");
    println!("5. Use async queues for request handling");
    println!("6. Track rate limit headers from provider");
}
