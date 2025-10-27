/// Maximum retry attempts for reconnection and HTTP requests
pub const MAX_RETRIES: u32 = 5;

/// WebSocket connection timeout in seconds
pub const WS_CONNECTION_TIMEOUT_SECS: u64 = 60;

/// Delay before reconnecting after disconnection in seconds
pub const RECONNECT_DELAY_SECS: u64 = 60;

/// Number of blocks to backfill at once
pub const BACKFILL_BATCH_SIZE: u64 = 9;  // Alchemy free tier allows 10 blocks per request

/// Transfer event signature
pub const TRANSFER_EVENT_SIGNATURE: &str = "Transfer(address,address,uint256)";

/// USDC Contract Address on Sepolia testnet
pub const CONTRACT_ADDRESS: &str = "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238";
