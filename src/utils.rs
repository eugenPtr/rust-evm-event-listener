use std::time::Duration;

/// Calculate exponential backoff duration capped at 60 seconds
/// Formula: 1000ms * 2^attempt, max 60000ms
pub fn calculate_backoff(attempt: u32) -> Duration {
    let base_ms = 1000u64.saturating_mul(2u64.pow(attempt)).min(60000);
    Duration::from_millis(base_ms)
}
