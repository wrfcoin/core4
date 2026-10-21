# Next Agent Handoff — Session 10: CLI Transaction Builder + Cross-Chain Trading

## Scope

Two issues that build the transaction and trading layer of the CLI:

1. **#289 — Interactive transaction builder** with dry-run, fee simulation, and risk checks
2. **#283 — WRF cross-chain liquidity** with AMM pools, bridge integration, price oracle, and `wrfcoin wallet swap`

Work on a fresh branch from `main`: `git fetch origin && git checkout -b feature/session10-cli-tx-trading origin/main`

---

## Codebase Overview

### CLI Architecture (`tools/cli/WrfcoinCLI/`)

Single Rust binary, clap 4.4 derive, tokio async. All commands return `anyhow::Result<Value>` with a `"summary"` key, emitted via `emit_success`/`emit_error`.

**Two wallet implementations exist (must unify):**

1. **New** (`src/commands/wallet.rs`): Clap enum with `Send { to, amount, fee: Option<f64>, memo: Option<String> }` — defined but handler just delegates to legacy code, dropping `fee` and `memo`.

2. **Legacy** (`src/cli/commands.rs`): Full handler with `Wallet` struct. `send_transaction()` hard-codes `fee = amount * 0.01` (1%), signs locally with Ed25519 via `ring`, saves to `~/.wrfcoin/wallet/wallet-state.json` with status `"signed-local"`. **Does NOT broadcast to the network** — no `ApiClient::send_transaction()` call.

**Bridge** (`src/commands/bridge.rs`): `Transfer` POSTs to `/bridges/transfer`. No route preview, no fee estimation, no dry-run.

**API Client** (`src/api.rs`):
```rust
send_transaction(from, to, amount, fee, signature) → POST /transactions
get_transaction(tx_id) → GET /transactions/{id}
get_pending_transactions() → GET /chain/pending-transactions
get_bridge_status() → GET /bridges
initiate_cross_chain_transfer(from_chain, to_chain, amount, recipient, signature) → POST /bridges/transfer
```
No swap, quote, or fee estimation endpoints exist yet.

**Crypto** (`src/crypto.rs`):
```rust
pub struct TransactionSigningData {
    pub tx_id: String,
    pub from: String,
    pub to: String,
    pub amount: f64,
    pub fee: f64,
    pub timestamp: String,
    pub tx_type: String,
    pub nonce: u64,  // currently: Utc::now().timestamp_millis() as u64
}
```
`WrfKeyPair::sign_transaction(tx_data)` validates `tx_data.from == self.address`, serializes to JSON, signs with Ed25519, returns base64. Address format: `"Wrf" + hex(SHA256(pubkey)[0..20])`.

### Backend Cross-Chain Server (`backend/server/enhanced_crosschain_server.ts`, port 8002)

Endpoints:
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/bridges` | All bridge protocols |
| GET | `/api/bridges/:protocol` | Single protocol detail |
| GET | `/api/networks` | Supported networks (ethereum/1, polygon/137, avalanche/43114) |
| POST | `/api/bridges/transfer` | Initiate transfer |
| GET | `/api/bridges/transactions/:id` | Transaction status |
| GET | `/api/bridges/stats` | 30-day aggregate stats |
| GET | `/api/bridges/history/:address` | Paginated history |

**Fee calculation:**
```typescript
function calculateBridgeFee(amount: number, protocol: BridgeProtocol): number {
    return protocol.fees.base + amount * protocol.fees.percentage / 100;
}
// axelar: base=0.1, percentage=0.1 → fee = 0.1 + amount*0.001
```

**Bridge protocols:** axelar, layerzero, wormhole, stargate, synapse — each with `supported_chains[]`, `fees`, `average_time`, `security_model`.

**CRITICAL BUG:** Server validates addresses with `/^0x[a-fA-F0-9]{40}$/` (EVM hex only). WRF native addresses (`Wrf...`) will be rejected. This must be fixed for WRF-native bridge support.

### Oracle Bridge Manager (`backend/server/oracle/enhanced-cross-chain-bridge-manager.ts`)

EventEmitter-based service (not HTTP). Coordinates cross-chain oracle requests.

Key capabilities:
- 13 supported chains (ethereum, polygon, bsc, avalanche, optimism, arbitrum, base, cosmos, osmosis, polkadot, kusama, near, solana)
- Proof types: Merkle, BLS aggregate, multi-signature
- Batch processing (up to 10 messages)
- Deduplication window (3600s)
- Health monitoring per bridge

### Smart Contracts — AMM Infrastructure

#### `CarbonCreditAMM.sol` — Constant Product Pool (x*y=k)

```solidity
contract CarbonCreditAMM is ERC20, AccessControl, ReentrancyGuard, Pausable {
    IERC20 public token0;
    IERC20 public token1;
    uint112 public reserve0;
    uint112 public reserve1;
    uint256 public feeBasisPoints = 30;           // 0.3%
    uint256 public protocolFeeBasisPoints = 5;    // 0.05%
    uint256 public maxSlippageBasisPoints = 500;  // 5%
    uint256 public minTimeBetweenTransactions = 12; // anti-MEV

    function getReserves() public view returns (uint112, uint112, uint32)
    function getSpotPrice() external view returns (uint256)  // reserve1/reserve0 * 1e18
    function getAmountOut(uint256 amountIn, uint256 reserveIn, uint256 reserveOut) public view returns (uint256)
    function getAmountIn(uint256 amountOut, uint256 reserveIn, uint256 reserveOut) public view returns (uint256)
    function calculatePriceImpact(uint256 amountIn, uint256 reserveIn, uint256 reserveOut) public view returns (uint256)
    function swap(uint256 amount0Out, uint256 amount1Out, address to, bytes calldata data) external
    function mint(address to) external returns (uint256 liquidity)
    function burn(address to) external returns (uint256 amount0, uint256 amount1)
}
```

#### `CarbonCreditAMMFactory.sol` — Pool Factory

```solidity
contract CarbonCreditAMMFactory is AccessControl, ReentrancyGuard, Pausable {
    mapping(address => mapping(address => mapping(uint256 => address))) public pools;
    // Fee tiers: 30 bps (standard), 50 bps (medium), 100 bps (high vol), 300 bps (exotic)
    function createPool(address tokenA, address tokenB, uint256 feeBasisPoints) external returns (address pool)
}
```

#### `CarbonCreditRouter.sol` — Multi-hop Swap Router

```solidity
contract CarbonCreditRouter is AccessControl, ReentrancyGuard, Pausable {
    CarbonCreditAMMFactory public immutable factory;
    address public immutable WETH;
    uint256 private constant MAX_ROUTE_HOPS = 3;
    uint256 public routingFeeBasisPoints = 5;     // 0.05%

    struct Route {
        address[] pools;
        address[] tokens;
        uint256[] feeTiers;
        uint256 expectedOutput;
        uint256 totalFee;
        uint256 priceImpact;  // basis points
    }

    struct SwapParams {
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        uint256 amountOutMin;  // slippage protection
        address to;
        uint256 deadline;
        uint256 maxHops;
    }

    // READ-ONLY QUOTE FUNCTIONS (use for dry-run):
    function getOptimalRoute(address tokenIn, address tokenOut, uint256 amountIn, uint256 maxHops)
        external view returns (Route memory)
    function getAmountsOut(uint256 amountIn, address[] calldata path, uint256[] calldata feeTiers)
        external view returns (uint256[] memory amounts)
    function getAmountsIn(uint256 amountOut, address[] calldata path, uint256[] calldata feeTiers)
        external view returns (uint256[] memory amounts)

    // EXECUTION:
    function swapExactTokensForTokens(SwapParams calldata) external returns (uint256 amountOut)
    function swapTokensForExactTokens(...) external returns (uint256 amountIn)
    function addLiquidity(LiquidityParams calldata) external returns (uint256, uint256, uint256)
    function removeLiquidity(...) external returns (uint256, uint256)
}
```

**Deadline enforcement:** `deadline >= block.timestamp && deadline <= block.timestamp + 1 hours`

#### Price Oracles

**`QualityOracle.sol`** — Multi-source price/quality aggregator:
- Providers: Chainlink, API3, Band Protocol, DIA, Tellor, Custom
- Circuit breaker at 3% deviation threshold
- Max staleness: 24 hours
- Weighted median aggregation with TWAQ smoothing

**`VolatilityOracle.sol`** — TWAV volatility tracker:
- Per-pair volatility states with price history
- Circuit breaker at >50% volatility
- Babylonian square root for standard deviation

#### `LiquidityPoolManager.sol` — Weighted Pool Orchestrator

```solidity
contract LiquidityPoolManager is AccessControl, ReentrancyGuard, Pausable {
    address public immutable usdcAddress;
    IVolatilityOracle public volatilityOracle;
    IQualityOracle public qualityOracle;
    uint256 public constant MIN_LIQUIDITY_THRESHOLD = 1000 * 1e6;  // 1000 USDC
    uint256 public protocolFeeBasisPoints = 10;  // 0.1%
}
```

---

## Issue #289: Interactive Transaction Builder

### Goal

`wrfcoin wallet send --interactive` with fee simulation, route/bridge preview, risk warnings, confirmation gate, and `--dry-run` that prints the exact payload and estimated outcomes without submitting.

### Critical Fixes Required First

Before building the interactive flow, fix two blocking bugs:

1. **Send must broadcast.** Currently `Wallet::send_transaction()` only signs locally. After signing, it must call `ApiClient::send_transaction()` to submit to the network. Return the server response (with real tx hash and block height) instead of `"signed-local"`.

2. **Unify wallet implementations.** The `commands/wallet.rs` `Send` variant has `fee` and `memo` fields that are dropped when mapping to the legacy handler. Wire these through properly.

### What to Build

#### 1. Fee Estimation Endpoint

**Add to `src/api.rs`:**
```rust
pub async fn estimate_fee(&self, from: &str, to: &str, amount: f64) -> Result<FeeEstimate>
    // GET /transactions/estimate-fee?from=&to=&amount=
```

```rust
pub struct FeeEstimate {
    pub base_fee: f64,
    pub priority_fee: f64,
    pub total_fee: f64,
    pub estimated_time: String,  // "~15 seconds"
    pub gas_price: Option<f64>,  // for EVM-bridged txns
}
```

If the API doesn't have this endpoint yet, fall back to the local 1% estimate but clearly label it as `"estimated (local)"`.

#### 2. Risk Checks

Create a risk assessment that runs before confirmation:

```rust
pub struct RiskAssessment {
    pub warnings: Vec<RiskWarning>,
    pub risk_level: RiskLevel,  // Low, Medium, High, Critical
}

pub struct RiskWarning {
    pub severity: RiskLevel,
    pub code: String,
    pub message: String,
}

pub enum RiskLevel { Low, Medium, High, Critical }
```

Risk checks to implement:

| Check | Severity | Condition |
|-------|----------|-----------|
| High fee | Medium | fee > 5% of amount |
| Very high fee | High | fee > 10% of amount |
| Unknown recipient | Medium | `to` address not in local wallet or recent history |
| Low balance after | High | remaining balance < 1 WRF after tx |
| Zero balance after | Critical | amount + fee >= balance |
| Large amount | Medium | amount > 1000 WRF (configurable via `high_value_threshold` in security.rs) |
| Dust amount | Low | amount < 0.001 WRF |
| Self-transfer | Low | from == to |
| Unconfirmed pending | Medium | >5 pending transactions from same address |

#### 3. `--interactive` Mode

**Modify `WalletCommands::Send`:**
```rust
Send {
    to: String,
    amount: f64,
    #[arg(long)] fee: Option<f64>,
    #[arg(long)] memo: Option<String>,
    #[arg(long, short = 'i', help = "Interactive mode with fee simulation and risk checks")]
    interactive: bool,
    #[arg(long, help = "Print payload without submitting")]
    dry_run: bool,
    #[arg(long, help = "Skip confirmation prompt")]
    yes: bool,
    #[arg(long, help = "Priority level: low, normal, high")]
    priority: Option<String>,
},
```

**Interactive flow:**

```
$ wrfcoin wallet send --interactive --to Wrf9b2e... --amount 500

Transaction Builder
═══════════════════

From:     Wrf8a3f...4d2c  (balance: 15,420.0000 WRF)
To:       Wrf9b2e...7f1a  (unknown address)
Amount:   500.0000 WRF
Fee:      0.5000 WRF (estimated, 0.1%)
Total:    500.5000 WRF
Remaining: 14,919.5000 WRF

⚠ Warning: Unknown recipient - not in your address book
  Verify the address carefully before proceeding.

Priority: Normal (~15 seconds confirmation)

Confirm transaction? [y/N]: y

✓ Transaction submitted
  Hash: 0x7a3f...8b2c
  Status: pending
  Block: (waiting for confirmation)
```

For JSON mode (`--json`):
```json
{
  "ok": true,
  "command": "wallet send",
  "data": {
    "phase": "confirmation",
    "transaction": {
      "from": "Wrf8a3f...4d2c",
      "to": "Wrf9b2e...7f1a",
      "amount": 500.0,
      "fee": 0.5,
      "total": 500.5,
      "remaining_balance": 14919.5,
      "priority": "normal",
      "estimated_time": "15s"
    },
    "risk_assessment": {
      "risk_level": "medium",
      "warnings": [
        {"severity": "medium", "code": "UNKNOWN_RECIPIENT", "message": "Not in address book"}
      ]
    },
    "result": {
      "hash": "0x7a3f...8b2c",
      "status": "pending"
    }
  }
}
```

#### 4. `--dry-run` Mode

When `--dry-run` is set, build and display the full transaction payload and risk assessment, but do NOT sign or broadcast:

```
$ wrfcoin wallet send --dry-run --to Wrf9b2e... --amount 500

DRY RUN — Transaction will NOT be submitted
════════════════════════════════════════════

Payload:
  tx_id:     tx_89f3a7b2c4...
  from:      Wrf8a3f...4d2c
  to:        Wrf9b2e...7f1a
  amount:    500.0000 WRF
  fee:       0.5000 WRF
  nonce:     1740240000000
  timestamp: 2026-02-22T15:30:00Z
  tx_type:   transfer

Fee Breakdown:
  Base fee:     0.4000 WRF
  Priority fee: 0.1000 WRF
  Total fee:    0.5000 WRF

Risk Assessment:
  ⚠ UNKNOWN_RECIPIENT: Address not in address book (medium)

Estimated Outcome:
  Sender balance:   15,420.0000 → 14,919.5000 WRF
  Recipient:        (balance unknown — not your address)
  Confirmation:     ~15 seconds
```

JSON dry-run output includes the full `TransactionSigningData` payload under `data.payload`.

#### 5. Bridge Route Preview

When the `to` address is on a different chain (detected by prefix or explicit `--chain` flag), show a bridge route preview:

```rust
// Add to WalletCommands::Send:
#[arg(long, help = "Destination chain (for cross-chain transfers)")]
chain: Option<String>,
```

If `--chain` is specified, show the bridge route before confirming:

```
Cross-Chain Transfer Preview
═════════════════════════════
Route:  WRFCoin → Ethereum (via Axelar)
Amount: 500 WRF
Bridge fee: 0.6 WRF (0.1 base + 0.5 at 0.1%)
Network fee: ~0.002 ETH
Estimated time: ~5 minutes
```

Query the bridge fee via `GET /api/bridges/:protocol` to get the fee structure.

### Files to Modify

| File | Changes |
|------|---------|
| `src/commands/wallet.rs` | Add `--interactive`, `--dry-run`, `--yes`, `--chain`, `--priority` to Send; implement interactive flow |
| `src/cli/commands.rs` | Fix `send_transaction()` to call `ApiClient::send_transaction()` for actual broadcast; wire `fee` and `memo` params |
| `src/api.rs` | Add `estimate_fee()` method |
| `src/main.rs` | Pass `ApiClient` to wallet handler for broadcast |

### Files to Create

| File | Purpose |
|------|---------|
| `src/commands/risk.rs` | `RiskAssessment`, `RiskWarning`, `assess_transaction()` function |

---

## Issue #283: WRF Cross-Chain Liquidity + `wrfcoin wallet swap`

### Goal

A functional trading path: WRF/USDC AMM pool, cross-chain bridge integration, WRF/USD price oracle, CLI `wrfcoin wallet swap` command.

### Architecture

The smart contracts already have the AMM infrastructure (CarbonCreditAMM, CarbonCreditRouter, oracles). What's missing is:
1. Backend REST endpoints that expose the contract's view functions (quotes, routes)
2. CLI swap command that calls those endpoints
3. WRF-specific pool deployment configuration
4. Documentation for the "cash out" path

### What to Build

#### 1. Backend Swap/Quote API Endpoints

**Create `backend/api/swap-api.ts`** (or add to existing router):

```typescript
// Quote endpoints (read-only, call contract view functions)
GET /api/swap/quote
  ?tokenIn=WRF&tokenOut=USDC&amountIn=500
  → { amountOut, route, priceImpact, fees, exchangeRate }

GET /api/swap/quote-reverse
  ?tokenIn=WRF&tokenOut=USDC&amountOut=100
  → { amountIn, route, priceImpact, fees, exchangeRate }

GET /api/swap/price
  ?pair=WRF/USDC
  → { price, lastUpdate, source, confidence, change24h }

GET /api/swap/pools
  → [{ pair, reserves, fee, tvl, volume24h, apy }]

GET /api/swap/pool/:pair
  → { token0, token1, reserve0, reserve1, fee, spotPrice, tvl }

// Execution endpoints
POST /api/swap/execute
  body: { tokenIn, tokenOut, amountIn, amountOutMin, deadline, signature }
  → { txHash, amountIn, amountOut, route, fees }

POST /api/swap/add-liquidity
  body: { tokenA, tokenB, amountA, amountB, amountAMin, amountBMin, deadline, signature }
  → { txHash, liquidity, amountA, amountB }
```

These endpoints should call the `CarbonCreditRouter` contract's view functions (`getOptimalRoute`, `getAmountsOut`, `getAmountsIn`) for quotes, and `swapExactTokensForTokens` for execution.

#### 2. CLI Swap Command

**Add to `WalletCommands` in `src/commands/wallet.rs`:**

```rust
/// Swap tokens via AMM
Swap {
    #[arg(long, required = true, help = "Token to sell")]
    from_token: String,  // e.g., "WRF"

    #[arg(long, required = true, help = "Token to buy")]
    to_token: String,    // e.g., "USDC"

    #[arg(long, required = true, help = "Amount to sell")]
    amount: f64,

    #[arg(long, help = "Minimum output amount (slippage protection)")]
    min_output: Option<f64>,

    #[arg(long, default_value = "1.0", help = "Max slippage percentage")]
    slippage: f64,

    #[arg(long, help = "Max hops for route finding")]
    max_hops: Option<u32>,

    #[arg(long, help = "Print quote without executing")]
    quote_only: bool,

    #[arg(long, help = "Skip confirmation")]
    yes: bool,
},
```

**Also add a `Price` command:**

```rust
/// Get current WRF price
Price {
    #[arg(default_value = "USDC")]
    quote_currency: String,
},
```

#### 3. Swap Flow Implementation

```rust
async fn handle_swap(
    api: &ApiClient,
    ctx: &ExecutionContext,
    from_token: &str,
    to_token: &str,
    amount: f64,
    slippage: f64,
    min_output: Option<f64>,
    max_hops: Option<u32>,
    quote_only: bool,
    yes: bool,
) -> Result<Value> {
    // 1. Get quote
    let quote = api.get_swap_quote(from_token, to_token, amount, max_hops).await?;

    // 2. Calculate min output from slippage if not explicitly set
    let min_out = min_output.unwrap_or_else(|| {
        quote.amount_out * (1.0 - slippage / 100.0)
    });

    // 3. Display quote
    //    Human: formatted table with route visualization
    //    JSON: structured quote object

    // 4. Risk checks
    //    - Price impact > 1%: warning
    //    - Price impact > 5%: high warning
    //    - Price impact > 10%: critical, suggest smaller trade
    //    - Insufficient balance
    //    - Pool liquidity low (tvl < 10x trade size)

    // 5. If quote_only, return here

    // 6. Confirm (unless --yes)

    // 7. Sign and execute
    let result = api.execute_swap(from_token, to_token, amount, min_out, deadline, signature).await?;

    // 8. Return result
}
```

**Human output for quote:**
```
Swap Quote
══════════

Sell:      500.0000 WRF
Buy:       ~112.50 USDC
Rate:      1 WRF = 0.2250 USDC
Route:     WRF → [Pool: 0.3% fee] → USDC
Impact:    0.12%
Fee:       0.15 WRF (0.03% routing + 0.3% pool)
Min out:   111.375 USDC (1% slippage protection)

Execute swap? [y/N]:
```

#### 4. Add API Methods to CLI Client

**Add to `src/api.rs`:**

```rust
pub struct SwapQuote {
    pub amount_out: f64,
    pub exchange_rate: f64,
    pub price_impact: f64,      // percentage
    pub route: Vec<String>,     // token path
    pub fees: SwapFees,
    pub min_output: f64,
}

pub struct SwapFees {
    pub routing_fee: f64,
    pub pool_fee: f64,
    pub total_fee: f64,
}

pub struct TokenPrice {
    pub price: f64,
    pub currency: String,
    pub last_update: String,
    pub source: String,
    pub change_24h: Option<f64>,
}

pub struct PoolInfo {
    pub pair: String,
    pub token0: String,
    pub token1: String,
    pub reserve0: f64,
    pub reserve1: f64,
    pub fee_bps: u32,
    pub spot_price: f64,
    pub tvl: f64,
    pub volume_24h: f64,
}

// Methods:
pub async fn get_swap_quote(&self, token_in: &str, token_out: &str, amount: f64, max_hops: Option<u32>) -> Result<SwapQuote>
    // GET /swap/quote?tokenIn=...&tokenOut=...&amountIn=...&maxHops=...

pub async fn execute_swap(&self, token_in: &str, token_out: &str, amount: f64, min_out: f64, deadline: u64, signature: &str) -> Result<Value>
    // POST /swap/execute

pub async fn get_token_price(&self, pair: &str) -> Result<TokenPrice>
    // GET /swap/price?pair=...

pub async fn get_pools(&self) -> Result<Vec<PoolInfo>>
    // GET /swap/pools
```

#### 5. Fix Bridge Address Validation

**Modify `backend/server/enhanced_crosschain_server.ts`:**

The server currently rejects WRF-native addresses (`Wrf...`). Add support:

```typescript
// Current (EVM only):
const isValidAddress = (addr: string) => /^0x[a-fA-F0-9]{40}$/.test(addr);

// Updated (support both):
const isValidAddress = (addr: string) =>
    /^0x[a-fA-F0-9]{40}$/.test(addr) ||   // EVM
    /^Wrf[a-fA-F0-9]{40}$/.test(addr);     // WRF native
```

Also update `CrossChainTransferRequest.fromAddress` and `toAddress` type from `EthereumAddress` to a union type or generic string with validation.

#### 6. WRF Token Configuration

**Create `backend/config/wrf-token.ts`** (or add to existing config):

```typescript
export const WRF_TOKEN_CONFIG = {
    symbol: "WRF",
    decimals: 18,
    // Contract addresses per chain (to be deployed)
    addresses: {
        wrfcoin: "native",                          // native token on WRFCoin chain
        ethereum: "0x...",                           // wrapped WRF on Ethereum
        polygon: "0x...",                            // wrapped WRF on Polygon
    },
    // AMM pool configurations
    pools: {
        "WRF/USDC": { feeTier: 30, minLiquidity: 1000 },
        "WRF/WETH": { feeTier: 50, minLiquidity: 500 },
    },
    // Bridge configuration
    bridgeable: true,
    supportedBridges: ["axelar", "layerzero", "wormhole"],
    // Price oracle
    oracleSources: ["chainlink", "band", "custom"],
};
```

### Files to Create

| File | Purpose |
|------|---------|
| `backend/api/swap-api.ts` | REST endpoints for swap quotes, execution, pool info |
| `backend/config/wrf-token.ts` | WRF token addresses, pool configs, bridge settings |
| `src/commands/risk.rs` | Risk assessment shared between send and swap (if not created in #289) |

### Files to Modify

| File | Changes |
|------|---------|
| `src/commands/wallet.rs` | Add `Swap`, `Price` subcommands |
| `src/api.rs` | Add `get_swap_quote()`, `execute_swap()`, `get_token_price()`, `get_pools()` |
| `src/main.rs` | Wire swap/price dispatching |
| `backend/server/enhanced_crosschain_server.ts` | Fix address validation to accept WRF native addresses |

### Files to Read (reference only)

| File | Why |
|------|-----|
| `contracts/CarbonCreditAMM.sol` | AMM pool interface, getAmountOut/getAmountIn |
| `contracts/CarbonCreditRouter.sol` | Router interface, getOptimalRoute, SwapParams |
| `contracts/CarbonCreditAMMFactory.sol` | Pool creation, fee tiers |
| `contracts/QualityOracle.sol` | Price feed interface |
| `contracts/VolatilityOracle.sol` | Volatility tracking |
| `contracts/LiquidityPoolManager.sol` | Pool orchestration, USDC integration |

---

## Implementation Order

**#289 must come first** — it fixes the broken `send_transaction()` broadcast and builds the risk assessment framework that #283's swap command reuses.

```
#289 (Transaction Builder)
  ├── Fix Wallet::send_transaction() to broadcast via ApiClient
  ├── Wire fee/memo params from new WalletCommands to handler
  ├── Add estimate_fee() API method
  ├── Create src/commands/risk.rs with risk assessment
  ├── Implement --interactive mode with dialoguer
  ├── Implement --dry-run mode
  ├── Add bridge route preview for --chain flag
  └── Tests

#283 (Cross-Chain Liquidity) — depends on #289
  ├── Create backend/api/swap-api.ts with quote/execute endpoints
  ├── Fix bridge address validation for WRF native addresses
  ├── Add swap/price API methods to CLI api.rs
  ├── Add Swap and Price subcommands to wallet.rs
  ├── Implement swap flow with quote, risk check, execute
  ├── Create WRF token configuration
  └── Tests
```

---

## Key Integration Points

### How Swap + Bridge + Send Connect

A "cash out" flow for a weather contributor:

```
1. wrfcoin wallet balance                    → See WRF balance
2. wrfcoin wallet price                      → Check WRF/USDC rate
3. wrfcoin wallet swap --from-token WRF --to-token USDC --amount 500 --quote-only
                                              → Preview swap quote
4. wrfcoin wallet swap --from-token WRF --to-token USDC --amount 500
                                              → Execute swap (WRF → USDC on WRFCoin chain)
5. wrfcoin bridge transfer --to-chain ethereum --to-address 0x... --amount 112 --asset USDC
                                              → Bridge USDC to Ethereum
6. (User withdraws USDC from Ethereum wallet to exchange/bank)
```

Each step is independently useful. The `--interactive` mode from #289 provides safety guardrails at steps 4 and 5.

---

## Build & Test

```bash
cd tools/cli/WrfcoinCLI

cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

# Test interactive send
cargo run -- wallet send --interactive --to Wrf9b2e... --amount 50

# Test dry-run
cargo run -- wallet send --dry-run --to Wrf9b2e... --amount 50 --json

# Test swap quote
cargo run -- wallet swap --from-token WRF --to-token USDC --amount 500 --quote-only

# Test price
cargo run -- wallet price USDC
```

## General Notes

- All CLI command handlers return `anyhow::Result<Value>` with a `"summary"` key.
- Use `emit_success`/`emit_error` for ALL output.
- Use `dialoguer::Confirm` for interactive prompts (check `ctx.non_interactive` / `--yes` to skip).
- The `--why` flag should trace through swap/bridge route decisions.
- Commit messages: `feat(cli):`, `fix(cli):`, `feat(api):`.
- Check `CLAUDE.md` at the project root for full platform context.
- The `enhanced_crosschain_server.ts` runs on port 8002, not the main API port 4001. The CLI may need to be configured to reach both.
