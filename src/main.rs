/* Example of subscribing and listening for USDC Transfer events on Sepolia testnet
    via WebSocket subscription with automatic reconnection & backfilling missed events.

   Implements the following features:
    - attempts to reconnect on WebSocket disconnections using exponential backoff
    - alternates between primary and fallback RPC URLs on each reconnection attempt
    - backfills any missed events during downtime by polling past logs from the RPC provider
    - backfills events in batches to avoid overwhelming the RPC provider (configurable via BACKFILL_BATCH_SIZE)
    - stores the last processed block number in memory

    For demonstration purposes:
    - The socket connection times out after WS_CONNECTION_TIMEOUT_SECS seconds to simulate disconnections & test reconnections
    - To test the backfilling logic, the program waits RECONNECT_DELAY_SECS seconds before reconnecting
    - last processed block number is initialized to the current block at startup

*/

mod utils;
mod provider_pool;

use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::Filter,
    sol,
    sol_types::SolEvent,
};
use eyre::{Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::timeout;
use utils::calculate_backoff;
use provider_pool::ProviderPool;

const MAX_RETRIES: u32 = 5;
const WS_CONNECTION_TIMEOUT_SECS: u64 = 60;  // 1 minute
const RECONNECT_DELAY_SECS: u64 = 60;         // 1 minute
const BACKFILL_BATCH_SIZE: u64 = 9;      // Number of blocks to backfill at once
const TRANSFER_EVENT_SIGNATURE: &str = "Transfer(address,address,uint256)";

// Define the ERC20 Transfer event using the sol! macro
// This generates a Transfer struct with from, to, and value fields
sol! {
    #[derive(Debug)]
    event Transfer(address indexed from, address indexed to, uint256 value);
}

#[derive(Serialize)]
struct TransferEventJson {
    from: String,
    to: String,
    value: String,
}

/// Application configuration loaded from environment variables
#[derive(Debug)]
struct Config {
    /// RPC URL (without protocol prefix)
    rpc_url: String,
    /// Fallback RPC URL (without protocol prefix)
    fallback_rpc_url: String,
    /// Contract address to monitor for events
    contract_address: String,
}

impl Config {
    fn from_env() -> Result<Self> {
        Ok(Config {
            rpc_url: std::env::var("RPC_URL")
                .context("RPC_URL environment variable not set")?,
            fallback_rpc_url: std::env::var("FALLBACK_RPC_URL")
                .context("FALLBACK_RPC_URL environment variable not set")?,
            contract_address: std::env::var("CONTRACT_ADDRESS")
                .context("CONTRACT_ADDRESS environment variable not set")?,
        })
    }
}

async fn backfill_events<P: Provider>(
    http_provider_pool: Arc<ProviderPool<P>>,
    last_processed_block: u64,
    current_block: u64,
    contract_address: &str,
) -> Result<()> {
    println!("Backfilling events from block {} to {}", last_processed_block, current_block);

    let address: Address = contract_address.parse()?;
    let mut from_block = last_processed_block + 1;

    while from_block <= current_block {
        let to_block = (from_block + BACKFILL_BATCH_SIZE).min(current_block);

        let filter = Filter::new()
            .address(address)
            .event("Transfer(address,address,uint256)")
            .from_block(from_block)
            .to_block(to_block);

        match http_provider_pool.with_retry(MAX_RETRIES, |provider| {
            let filter = filter.clone();
            async move {
                provider.get_logs(&filter).await
            }
        }).await {
            Ok(logs) => {
                println!("Backfilled {} events from blocks {}-{}", logs.len(), from_block, to_block);

                for log in logs {
                    match Transfer::decode_log(log.as_ref()) {
                        Ok(event) => {
                            let json_event = TransferEventJson {
                                from: format!("{:?}", event.from),
                                to: format!("{:?}", event.to),
                                value: event.value.to_string(),
                            };
                            if let Ok(json) = serde_json::to_string(&json_event) {
                                println!("[Block {}] Backfill Transfer: {}", log.block_number.unwrap(), json);
                            }
                        }
                        Err(e) => eprintln!("Decode error: {:?}", e),
                    }
                }
            }
            Err(e) => {
                eprintln!("Backfill error for blocks {}-{}: {:?}", from_block, to_block, e);
            }
        }

        from_block = to_block + 1;
    }

    Ok(())
}

async fn connect_ws(
    ws_provider: &impl Provider,
    contract_address: &str,
) -> eyre::Result<std::pin::Pin<Box<dyn futures_util::stream::Stream<Item = alloy::rpc::types::Log> + Send + 'static>>> {
    let contract_address: Address = contract_address
        .parse()
        .context("Invalid contract address")?;

    let filter = Filter::new()
        .address(contract_address)
        .event(TRANSFER_EVENT_SIGNATURE);

    let subscription = ws_provider
        .subscribe_logs(&filter)
        .await
        .context("Failed to subscribe to logs")?;

    let stream = subscription.into_stream().boxed();

    Ok(stream)
}

fn initialize_http_provider_pool(config: &Config) -> Arc<ProviderPool<impl Provider>> {
    let primary_http_url = format!("https://{}", config.rpc_url);
    let fallback_http_url = format!("https://{}", config.fallback_rpc_url);

    let primary_http_provider = ProviderBuilder::new()
        .connect_http(primary_http_url.parse().unwrap());
    let fallback_http_provider = ProviderBuilder::new()
        .connect_http(fallback_http_url.parse().unwrap());

    Arc::new(ProviderPool::new(vec![primary_http_provider, fallback_http_provider]))
}

/*
    Cycles between primary and fallback WebSocket RPC URLs based on the attempt number
*/
async fn get_ws_provider(
    config: &Config,
    attempt: &u32,
) -> impl Provider {

    let rpc_url = if *attempt % 2 == 0 {
        &config.rpc_url
    } else {
        &config.fallback_rpc_url
    };

    let ws_url = format!("wss://{}", rpc_url);
    let ws = WsConnect::new(&ws_url);
    ProviderBuilder::new()
        .connect_ws(ws)
        .await
        .context("Failed to connect to WebSocket")
        .unwrap()
}

/*
    Fetch the latest block processed by our event listener
    For this POC, we initialize it to the current block number from the RPC
    !!! In production, this value would be fetched from a database
*/
async fn get_last_processed_block<P: Provider>(
    http_provider_pool: Arc<ProviderPool<P>>
) -> Arc<Mutex<u64>> {
    let current_block = http_provider_pool
        .with_retry(MAX_RETRIES, |provider| async move {
            provider.get_block_number().await
        })
        .await
        .context("Failed to fetch latest block via HTTP").unwrap();

    Arc::new(Mutex::new(current_block))
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present (helpful for local development)
    let _ = dotenvy::dotenv();

    // Load and validate configuration at startup
    let config = Config::from_env()
        .context("Failed to load configuration")?;

    println!("Configuration loaded successfully");
    println!("RPC URL: {}", config.rpc_url);
    println!("Fallback RPC URL: {}", config.fallback_rpc_url);

    // Initialize HTTP provider pool once for the entire application lifecycle
    let http_provider_pool = initialize_http_provider_pool(&config);

    let last_processed_block = get_last_processed_block(http_provider_pool.clone()).await;

    let mut attempt = 0;
    loop {
        if attempt >= MAX_RETRIES {
            println!("✗ Max retries ({}) exceeded", MAX_RETRIES);
            break;
        }

        let ws_provider = get_ws_provider(&config, &attempt).await;

        let mut event_stream = match connect_ws(&ws_provider, &config.contract_address).await {
            Ok(stream) => {
                attempt = 0; // Reset attempt counter on successful connection
                stream
            }
            Err(e) => {
                println!("✗ Reconnection attempt {} failed: {:?}", attempt + 1, e);
                let backoff = calculate_backoff(attempt);
                println!("⟳ Retrying in {:?}", backoff);
                tokio::time::sleep(backoff).await;
                attempt += 1;
                continue;
            }
        };

        let current_block = http_provider_pool
            .with_retry(MAX_RETRIES, |provider| async move {
                provider.get_block_number().await
            })
            .await
            .context("Failed to fetch latest block via HTTP")?;

        println!("Websocket connection established at block: {}", current_block);

        // On successful connection, check for missed events
        let backfill_to = current_block;
        let backfill_from = *last_processed_block.lock().unwrap();
        if backfill_to > backfill_from {
            let address = config.contract_address.clone();
            let pool_clone = http_provider_pool.clone();

            // Backfill task is handed to the runtime. It is executed asynchronously evenif the loop continues due to ws disconnection
            tokio::spawn(async move {
                if let Err(e) = backfill_events(pool_clone, backfill_from, backfill_to, &address).await {
                    eprintln!("Backfill error: {:?}", e);
                }
            });
        }

        // Listen for events with a timeout
        let timeout_duration = Duration::from_secs(WS_CONNECTION_TIMEOUT_SECS);
        let listen_result = timeout(timeout_duration, async {
            while let Some(log) = event_stream.next().await {
                let transfer = log.log_decode::<Transfer>()
                    .context("Failed to decode Transfer event")?;

                let json_event = TransferEventJson {
                    from: format!("{:?}", transfer.inner.from),
                    to: format!("{:?}", transfer.inner.to),
                    value: transfer.inner.value.to_string(),
                };

                if let Ok(json) = serde_json::to_string(&json_event) {
                    println!("[Block {}] Live Transfer: {}", log.block_number.unwrap(), json);
                }

                let block_number = log.block_number.unwrap();
                let mut last_block = last_processed_block.lock().unwrap();
                *last_block = block_number;
            }
            Ok::<(), eyre::Report>(())
        }).await;

        match listen_result {
            Ok(Ok(())) => {
                println!("⚠ Stream ended naturally");
            }
            Ok(Err(e)) => {
                eprintln!("✗ Error during listening: {:?}", e);
            }
            Err(_) => {
                println!("⏱ Connection timeout after {} seconds", WS_CONNECTION_TIMEOUT_SECS);
            }
        }

        println!("Last processed block: {}", *last_processed_block.lock().unwrap());

        // Wait before reconnecting
        println!("⏸ Waiting {} seconds before reconnecting...", RECONNECT_DELAY_SECS);
        tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
    }

    Ok(())
}