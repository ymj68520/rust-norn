/**
 * Rate-Limiting Utilities Example
 *
 * This example demonstrates various rate-limiting strategies for blockchain RPC calls:
 *
 * Rate-Limiting Strategies:
 * 1. Token Bucket: Refill tokens at a fixed rate
 * 2. Sliding Window: Track requests in a time window
 * 3. Adaptive Rate-Limit: Adjust rate based on response codes
 * 4. Per-Method Limits: Different limits for different RPC methods
 * 5. Backoff Strategy: Exponential backoff on rate-limit errors
 *
 * Why rate-limiting is important:
 * - Most RPC providers have rate limits
 * - Prevents overwhelming the node
 * - Protects against accidental DoS
 * - Manages costs on paid RPC services
 * - Improves reliability and consistency
 */

/**
 * Token bucket rate limiter
 * Allows N requests per duration, with optional burst capacity
 */
class TokenBucket {
  constructor(capacity, refillRate) {
    this.capacity = capacity;
    this.tokens = capacity;
    this.refillRate = refillRate; // tokens per second
    this.lastRefill = Date.now();
  }

  /**
   * Try to take tokens, returns true if successful
   */
  tryAcquire(numTokens = 1) {
    // Calculate tokens to add based on time passed
    const now = Date.now();
    const elapsed = (now - this.lastRefill) / 1000; // convert to seconds
    const tokensToAdd = Math.floor(elapsed * this.refillRate);

    if (tokensToAdd > 0) {
      this.tokens = Math.min(this.tokens + tokensToAdd, this.capacity);
      this.lastRefill = now;
    }

    if (this.tokens >= numTokens) {
      this.tokens -= numTokens;
      return true;
    } else {
      return false;
    }
  }

  /**
   * Get time to wait before next token is available (in milliseconds)
   */
  timeUntilAvailableMs() {
    if (this.tokens > 0) {
      return 0;
    }

    const timeToNextToken = 1.0 / this.refillRate;
    return Math.floor(timeToNextToken * 1000);
  }
}

/**
 * Sliding window rate limiter
 * Tracks requests in a time window
 */
class SlidingWindow {
  constructor(maxRequests, windowDurationMs) {
    this.maxRequests = maxRequests;
    this.windowDurationMs = windowDurationMs;
    this.requests = [];
  }

  /**
   * Try to make a request
   */
  tryAcquire() {
    const now = Date.now();

    // Remove old requests outside the window
    this.requests = this.requests.filter(
      time => now - time <= this.windowDurationMs
    );

    // Check if we can add a new request
    if (this.requests.length < this.maxRequests) {
      this.requests.push(now);
      return true;
    } else {
      return false;
    }
  }

  /**
   * Get current request count in the window
   */
  currentRequests() {
    const now = Date.now();
    this.requests = this.requests.filter(
      time => now - time <= this.windowDurationMs
    );
    return this.requests.length;
  }
}

/**
 * Per-method rate limiter
 * Different limits for different RPC methods
 */
class PerMethodLimiter {
  constructor() {
    this.limiters = new Map();
  }

  /**
   * Register a method with rate limit
   *
   * @param {string} method - RPC method name
   * @param {number} capacity - token bucket capacity
   * @param {number} refillRate - tokens per second
   */
  registerMethod(method, capacity, refillRate) {
    this.limiters.set(method, new TokenBucket(capacity, refillRate));
  }

  /**
   * Try to acquire for a method
   */
  tryAcquire(method) {
    if (this.limiters.has(method)) {
      return this.limiters.get(method).tryAcquire();
    } else {
      // Method not registered, allow it
      return true;
    }
  }
}

/**
 * Exponential backoff helper
 */
class ExponentialBackoff {
  constructor(initialDelayMs, maxDelayMs) {
    this.initialDelayMs = initialDelayMs;
    this.maxDelayMs = maxDelayMs;
    this.multiplier = 2.0;
    this.attempt = 0;
  }

  /**
   * Get delay for current attempt (in milliseconds)
   */
  getDelayMs() {
    const delay = Math.floor(
      this.initialDelayMs * Math.pow(this.multiplier, this.attempt)
    );
    return Math.min(delay, this.maxDelayMs);
  }

  /**
   * Increment attempt counter
   */
  nextAttempt() {
    this.attempt += 1;
  }

  /**
   * Reset attempt counter
   */
  reset() {
    this.attempt = 0;
  }

  /**
   * Check if max retries reached (arbitrary limit of 10 attempts)
   */
  shouldRetry() {
    return this.attempt < 10;
  }
}

/**
 * Adaptive rate limiter
 * Adjusts rate based on error responses
 */
class AdaptiveRateLimiter {
  constructor(initialRate, minRate, maxRate) {
    this.currentRate = initialRate;
    this.minRate = minRate;
    this.maxRate = maxRate;
    this.decreaseFactor = 0.5;   // Halve on error
    this.increaseFactor = 1.1;   // Increase 10% on success
  }

  /**
   * Record a successful request
   */
  recordSuccess() {
    this.currentRate = Math.min(
      this.currentRate * this.increaseFactor,
      this.maxRate
    );
  }

  /**
   * Record a failed request (rate limit hit)
   */
  recordRateLimitError() {
    this.currentRate = Math.max(
      this.currentRate * this.decreaseFactor,
      this.minRate
    );
  }

  /**
   * Get current rate
   */
  getCurrentRate() {
    return this.currentRate;
  }

  /**
   * Get token bucket based on current rate
   */
  getTokenBucket() {
    const rate = this.getCurrentRate();
    const capacity = Math.max(Math.ceil(rate), 1);
    return new TokenBucket(capacity, rate);
  }
}

/**
 * Main example demonstrating rate-limiting strategies
 */
function main() {
  console.log('=== Rate-Limiting Utilities Examples ===\n');

  // Example 1: Token Bucket Rate Limiter
  console.log('1. Token Bucket Rate Limiter:');
  console.log('   Allows 10 requests per second with burst capacity of 20');
  const tb = new TokenBucket(20, 10.0);

  for (let i = 1; i <= 25; i++) {
    if (tb.tryAcquire()) {
      console.log(`   Request ${i} - ALLOWED`);
    } else {
      const waitMs = tb.timeUntilAvailableMs();
      console.log(`   Request ${i} - DENIED (wait ${waitMs}ms)`);
    }
  }

  // Example 2: Sliding Window Rate Limiter
  console.log('\n2. Sliding Window Rate Limiter:');
  console.log('   Max 5 requests per 1 second window');
  const sw = new SlidingWindow(5, 1000);

  for (let i = 1; i <= 7; i++) {
    if (sw.tryAcquire()) {
      console.log(
        `   Request ${i} - ALLOWED (total in window: ${sw.currentRequests()})`
      );
    } else {
      console.log(`   Request ${i} - DENIED (window full)`);
    }
  }

  // Example 3: Per-Method Rate Limiting
  console.log('\n3. Per-Method Rate Limiting:');
  console.log('   Different limits for different RPC methods');
  const pm = new PerMethodLimiter();

  // Register methods with different rates
  pm.registerMethod('eth_call', 100, 50.0);           // 50 per second, burst 100
  pm.registerMethod('eth_getBalance', 50, 20.0);     // 20 per second, burst 50
  pm.registerMethod('eth_sendRawTransaction', 10, 5.0); // 5 per second, burst 10

  console.log(`   eth_call: ${pm.tryAcquire('eth_call') ? '✓ ALLOWED' : '✗ DENIED'}`);
  console.log(
    `   eth_getBalance: ${pm.tryAcquire('eth_getBalance') ? '✓ ALLOWED' : '✗ DENIED'}`
  );
  console.log(
    `   eth_sendRawTransaction: ${pm.tryAcquire('eth_sendRawTransaction') ? '✓ ALLOWED' : '✗ DENIED'}`
  );

  // Example 4: Exponential Backoff
  console.log('\n4. Exponential Backoff Strategy:');
  console.log('   Backoff delays for retries after rate limit');
  const backoff = new ExponentialBackoff(100, 30000);

  for (let attempt = 0; attempt < 5; attempt++) {
    console.log(`   Attempt ${attempt + 1} - Wait ${backoff.getDelayMs()}ms`);
    backoff.nextAttempt();
  }

  // Example 5: Adaptive Rate Limiting
  console.log('\n5. Adaptive Rate Limiting:');
  console.log('   Adjusts rate based on error responses');
  const adaptive = new AdaptiveRateLimiter(50.0, 5.0, 100.0);

  console.log(`   Initial rate: ${adaptive.getCurrentRate().toFixed(1)} req/sec`);

  // Simulate successes
  for (let i = 0; i < 3; i++) {
    adaptive.recordSuccess();
  }
  console.log(
    `   After 3 successes: ${adaptive.getCurrentRate().toFixed(1)} req/sec`
  );

  // Simulate rate limit error
  adaptive.recordRateLimitError();
  console.log(
    `   After rate limit error: ${adaptive.getCurrentRate().toFixed(1)} req/sec`
  );

  // Example 6: Common rate limit patterns for popular providers
  console.log('\n6. Rate Limit Patterns for Popular Providers:');
  console.log('   Infura: 100 requests/sec');
  console.log('   Alchemy: 300 requests/second (up to 3,000 CU/sec)');
  console.log('   QuickNode: 100-200 requests/sec depending on plan');
  console.log('   Etherscan: 5 calls/second free tier');

  // Example 7: Best practices
  console.log('\n=== Best Practices ===');
  console.log('✓ Always implement rate limiting for production applications');
  console.log('✓ Use token bucket for predictable, consistent rates');
  console.log('✓ Use sliding window for strict per-second limits');
  console.log('✓ Implement exponential backoff for retries');
  console.log('✓ Monitor 429 (Too Many Requests) responses');
  console.log('✓ Consider using adaptive limits for dynamic environments');
  console.log('✓ Batch requests when possible to reduce total calls');
  console.log('✓ Cache read-only data to avoid repeated queries');

  console.log('\n=== Implementation Tips ===');
  console.log('1. Start with token bucket (most flexible)');
  console.log('2. Combine with per-method limits for fine-grained control');
  console.log('3. Add exponential backoff for resilience');
  console.log('4. Monitor actual rate and adjust limits as needed');
  console.log('5. Use async queues for request handling');
  console.log('6. Track rate limit headers from provider');
}

main();
