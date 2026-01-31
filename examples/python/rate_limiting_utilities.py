"""
Rate-Limiting Utilities Example

This example demonstrates various rate-limiting strategies for blockchain RPC calls:

Rate-Limiting Strategies:
1. Token Bucket: Refill tokens at a fixed rate
2. Sliding Window: Track requests in a time window
3. Adaptive Rate-Limit: Adjust rate based on response codes
4. Per-Method Limits: Different limits for different RPC methods
5. Backoff Strategy: Exponential backoff on rate-limit errors

Why rate-limiting is important:
- Most RPC providers have rate limits
- Prevents overwhelming the node
- Protects against accidental DoS
- Manages costs on paid RPC services
- Improves reliability and consistency
"""

import time
from collections import deque
from threading import Lock
from typing import Dict
import asyncio


class TokenBucket:
    """Token bucket rate limiter
    Allows N requests per duration, with optional burst capacity
    """

    def __init__(self, capacity: int, refill_rate: float):
        """
        Create a new token bucket

        Args:
            capacity: max tokens (also burst capacity)
            refill_rate: tokens per second
        """
        self.capacity = capacity
        self.tokens = capacity
        self.refill_rate = refill_rate
        self.last_refill = time.time()
        self.lock = Lock()

    def try_acquire(self, num_tokens: int = 1) -> bool:
        """Try to take tokens, returns true if successful"""
        with self.lock:
            # Calculate tokens to add based on time passed
            now = time.time()
            elapsed = now - self.last_refill
            tokens_to_add = int(elapsed * self.refill_rate)

            if tokens_to_add > 0:
                self.tokens = min(self.tokens + tokens_to_add, self.capacity)
                self.last_refill = now

            if self.tokens >= num_tokens:
                self.tokens -= num_tokens
                return True
            else:
                return False

    def time_until_available_ms(self) -> int:
        """Get time to wait before next token is available (in milliseconds)"""
        with self.lock:
            if self.tokens > 0:
                return 0

            time_to_next_token = 1.0 / self.refill_rate
            return int(time_to_next_token * 1000)


class SlidingWindow:
    """Sliding window rate limiter
    Tracks requests in a time window
    """

    def __init__(self, max_requests: int, window_duration: float):
        """
        Create a new sliding window limiter

        Args:
            max_requests: max requests in the window
            window_duration: time window duration in seconds
        """
        self.max_requests = max_requests
        self.window_duration = window_duration
        self.requests = deque()
        self.lock = Lock()

    def try_acquire(self) -> bool:
        """Try to make a request"""
        with self.lock:
            now = time.time()

            # Remove old requests outside the window
            while self.requests and now - self.requests[0] > self.window_duration:
                self.requests.popleft()

            # Check if we can add a new request
            if len(self.requests) < self.max_requests:
                self.requests.append(now)
                return True
            else:
                return False

    def current_requests(self) -> int:
        """Get current request count in the window"""
        with self.lock:
            now = time.time()

            # Clean up old requests
            while self.requests and now - self.requests[0] > self.window_duration:
                self.requests.popleft()

            return len(self.requests)


class PerMethodLimiter:
    """Per-method rate limiter
    Different limits for different RPC methods
    """

    def __init__(self):
        """Create a new per-method limiter"""
        self.limiters: Dict[str, TokenBucket] = {}
        self.lock = Lock()

    def register_method(self, method: str, capacity: int, refill_rate: float):
        """Register a method with rate limit

        Args:
            method: RPC method name
            capacity: token bucket capacity
            refill_rate: tokens per second
        """
        with self.lock:
            self.limiters[method] = TokenBucket(capacity, refill_rate)

    def try_acquire(self, method: str) -> bool:
        """Try to acquire for a method"""
        with self.lock:
            if method in self.limiters:
                return self.limiters[method].try_acquire()
            else:
                # Method not registered, allow it
                return True


class ExponentialBackoff:
    """Exponential backoff helper"""

    def __init__(self, initial_delay_ms: int, max_delay_ms: int):
        """Create a new exponential backoff"""
        self.initial_delay_ms = initial_delay_ms
        self.max_delay_ms = max_delay_ms
        self.multiplier = 2.0
        self.attempt = 0

    def get_delay_ms(self) -> int:
        """Get delay for current attempt (in milliseconds)"""
        delay = int(self.initial_delay_ms * (self.multiplier**self.attempt))
        return min(delay, self.max_delay_ms)

    def next_attempt(self):
        """Increment attempt counter"""
        self.attempt += 1

    def reset(self):
        """Reset attempt counter"""
        self.attempt = 0

    def should_retry(self) -> bool:
        """Check if max retries reached (arbitrary limit of 10 attempts)"""
        return self.attempt < 10


class AdaptiveRateLimiter:
    """Adaptive rate limiter
    Adjusts rate based on error responses
    """

    def __init__(self, initial_rate: float, min_rate: float, max_rate: float):
        """Create a new adaptive rate limiter"""
        self.current_rate = initial_rate
        self.min_rate = min_rate
        self.max_rate = max_rate
        self.decrease_factor = 0.5  # Halve on error
        self.increase_factor = 1.1  # Increase 10% on success
        self.lock = Lock()

    def record_success(self):
        """Record a successful request"""
        with self.lock:
            self.current_rate = min(
                self.current_rate * self.increase_factor, self.max_rate
            )

    def record_rate_limit_error(self):
        """Record a failed request (rate limit hit)"""
        with self.lock:
            self.current_rate = max(
                self.current_rate * self.decrease_factor, self.min_rate
            )

    def get_current_rate(self) -> float:
        """Get current rate"""
        with self.lock:
            return self.current_rate

    def get_token_bucket(self) -> TokenBucket:
        """Get token bucket based on current rate"""
        rate = self.get_current_rate()
        capacity = max(int(rate), 1)
        return TokenBucket(capacity, rate)


def main():
    """Main example demonstrating rate-limiting strategies"""
    print("=== Rate-Limiting Utilities Examples ===\n")

    # Example 1: Token Bucket Rate Limiter
    print("1. Token Bucket Rate Limiter:")
    print("   Allows 10 requests per second with burst capacity of 20")
    tb = TokenBucket(20, 10.0)

    for i in range(1, 26):
        if tb.try_acquire():
            print(f"   Request {i} - ALLOWED")
        else:
            wait_ms = tb.time_until_available_ms()
            print(f"   Request {i} - DENIED (wait {wait_ms}ms)")

    # Example 2: Sliding Window Rate Limiter
    print("\n2. Sliding Window Rate Limiter:")
    print("   Max 5 requests per 1 second window")
    sw = SlidingWindow(5, 1.0)

    for i in range(1, 8):
        if sw.try_acquire():
            print(
                f"   Request {i} - ALLOWED (total in window: {sw.current_requests()})"
            )
        else:
            print(f"   Request {i} - DENIED (window full)")

    # Example 3: Per-Method Rate Limiting
    print("\n3. Per-Method Rate Limiting:")
    print("   Different limits for different RPC methods")
    pm = PerMethodLimiter()

    # Register methods with different rates
    pm.register_method("eth_call", 100, 50.0)  # 50 per second, burst 100
    pm.register_method("eth_getBalance", 50, 20.0)  # 20 per second, burst 50
    pm.register_method("eth_sendRawTransaction", 10, 5.0)  # 5 per second, burst 10

    print(f"   eth_call: {'✓ ALLOWED' if pm.try_acquire('eth_call') else '✗ DENIED'}")
    print(
        f"   eth_getBalance: {'✓ ALLOWED' if pm.try_acquire('eth_getBalance') else '✗ DENIED'}"
    )
    print(
        f"   eth_sendRawTransaction: {'✓ ALLOWED' if pm.try_acquire('eth_sendRawTransaction') else '✗ DENIED'}"
    )

    # Example 4: Exponential Backoff
    print("\n4. Exponential Backoff Strategy:")
    print("   Backoff delays for retries after rate limit")
    backoff = ExponentialBackoff(100, 30000)

    for attempt in range(5):
        print(f"   Attempt {attempt + 1} - Wait {backoff.get_delay_ms()}ms")
        backoff.next_attempt()

    # Example 5: Adaptive Rate Limiting
    print("\n5. Adaptive Rate Limiting:")
    print("   Adjusts rate based on error responses")
    adaptive = AdaptiveRateLimiter(50.0, 5.0, 100.0)

    print(f"   Initial rate: {adaptive.get_current_rate():.1f} req/sec")

    # Simulate successes
    for _ in range(3):
        adaptive.record_success()
    print(f"   After 3 successes: {adaptive.get_current_rate():.1f} req/sec")

    # Simulate rate limit error
    adaptive.record_rate_limit_error()
    print(f"   After rate limit error: {adaptive.get_current_rate():.1f} req/sec")

    # Example 6: Common rate limit patterns for popular providers
    print("\n6. Rate Limit Patterns for Popular Providers:")
    print("   Infura: 100 requests/sec")
    print("   Alchemy: 300 requests/second (up to 3,000 CU/sec)")
    print("   QuickNode: 100-200 requests/sec depending on plan")
    print("   Etherscan: 5 calls/second free tier")

    # Example 7: Best practices
    print("\n=== Best Practices ===")
    print("✓ Always implement rate limiting for production applications")
    print("✓ Use token bucket for predictable, consistent rates")
    print("✓ Use sliding window for strict per-second limits")
    print("✓ Implement exponential backoff for retries")
    print("✓ Monitor 429 (Too Many Requests) responses")
    print("✓ Consider using adaptive limits for dynamic environments")
    print("✓ Batch requests when possible to reduce total calls")
    print("✓ Cache read-only data to avoid repeated queries")

    print("\n=== Implementation Tips ===")
    print("1. Start with token bucket (most flexible)")
    print("2. Combine with per-method limits for fine-grained control")
    print("3. Add exponential backoff for resilience")
    print("4. Monitor actual rate and adjust limits as needed")
    print("5. Use async queues for request handling")
    print("6. Track rate limit headers from provider")


if __name__ == "__main__":
    main()
