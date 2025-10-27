# Rust Event Listener

An EVM event listener built in Rust that monitors USDC Transfer events on the Sepolia testnet with automatic reconnection, exponential backoff, and event backfilling.

## Features

- **WebSocket Event Subscription**: Real-time monitoring of Transfer events via WebSocket
- **Automatic Reconnection**: Reconnects on WebSocket disconnections using exponential backoff
- **Provider Failover**: Alternates between primary and fallback RPC providers on connection failures
- **Event Backfilling**: Automatically backfills missed events during downtime by querying historical logs
- **HTTP Provider Pool**: Load-balanced HTTP requests with automatic retry and provider rotation

## Prerequisites

- **Rust**: Install from [rust-lang.org](https://www.rust-lang.org/tools/install)
- **Ethereum RPC Access**: Obtain API keys from providers like [Alchemy](https://www.alchemy.com/) or [Infura](https://infura.io/)

## Setup

### 1. Configure Environment Variables

Create a `.env` file in the project root:

```bash
cp .env.example .env
```

Edit `.env` and add your RPC URLs:

**Note**: Protocol prefixes (`wss://` and `https://`) are automatically added by the application.

### 3. Adjust Constants (Optional)

Application constants are defined in `src/constants.rs`. You can modify these values to customize behavior:

## Running the Application

### Build and Run

```bash
cargo run
```

## How It Works

See comments at the top of the source files

## License

This project is provided as-is for educational and demonstration purposes.
