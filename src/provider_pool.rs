/*
   ProviderPool: A pool of HTTP providers with round-robin selection and retry logic

   How it works:
   1. Each request gets a starting provider via atomic counter increment
   2. Attempts the operation on that provider up to max_retries times with exponential backoff
   3. If all retries fail, rotates to the next provider and repeats
   4. Cycles through all providers until success or all providers exhausted

   Pros:
    - Can avoid overloading a single provider and rate limits
    - Retries failed requests automatically
    - Falls back to other providers on persistent failures
*/

use alloy::providers::Provider;
use eyre::Result;
use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};
use crate::utils::calculate_backoff;

pub struct ProviderPool<P> {
    providers: Vec<Arc<P>>,
    current_index: AtomicUsize,
}

impl<P: Provider> ProviderPool<P> {
    pub fn new(providers: Vec<P>) -> Self {
        Self {
            providers: providers.into_iter().map(Arc::new).collect(),
            current_index: AtomicUsize::new(0),
        }
    }

    pub async fn with_retry<T, E, F, Fut>(&self, max_retries: u32, operation: F) -> Result<T>
    where
        F: Fn(Arc<P>) -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Debug,
        T: Send,
    {
        let start_index = self.current_index.fetch_add(1, Ordering::Relaxed) % self.providers.len();

        // Try each provider in round-robin order
        for provider_offset in 0..self.providers.len() {
            let provider_index = (start_index + provider_offset) % self.providers.len();
            let provider = self.providers[provider_index].clone();

            // Retry this provider up to max_retries times
            for attempt in 0..max_retries {
                match operation(provider.clone()).await {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        println!(
                            "⚠ HTTP request failed on provider {} (attempt {}): {:?}.",
                            provider_index, attempt + 1, e,
                        );

                        if attempt < max_retries - 1 {
                            let backoff = calculate_backoff(attempt);
                            println!(
                                "⚠ Retrying in {:?}",
                                backoff
                            );
                            tokio::time::sleep(backoff).await;
                        }
                    }
                }
            }
        }

        eyre::bail!("Unable to complete request after trying all {} providers with {} retries each", self.providers.len(), max_retries)
    }
}
