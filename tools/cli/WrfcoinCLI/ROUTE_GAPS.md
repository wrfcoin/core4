# CLI route truth — r20 SOL-I, Refs #3247

The default CLI client targets the node. A route mounted by the backend is
not automatically a node route, and needs an authenticated backend client.
The node route source is `native-chain/src/bin/launch_api/routes/mod.rs`.

| Command | Target | Behavior in this tranche |
| --- | --- | --- |
| `nft list [--owner ADDRESS]` | Node `GET /nft/list?owner=ADDRESS` | Real bounded listing; the node's `items`, `total`, `limit`, `offset` response is returned as `listing` |
| `nft show ID` | Node `GET /nft/{id}` | Existing read retained |
| `nft list --nft-type TYPE` | No node filter for this field | Hidden option; explicit not-wired result before I/O |
| `nft create/submit/categories/add-category/register-verifier/list-verifiers/verify` | CLI computational-NFT routes absent from the node | Hidden; explicit not-wired result before I/O |
| `weather submit/submit-batch/submit-wind/create-nft/query-history/sync` | Legacy node-targeted weather paths absent | Hidden; explicit not-wired result before files, keys, queue or HTTP |
| `batch run` (file/stdin, with or without `--fail-fast`) | Only legacy weather submit/history were allowlisted | Hidden; typed refusal before reading input or HTTP; placeholder-signature dispatcher retired |
| `weather queue/queue-inspect/queue-purge` | Local SQLite queue | Existing local operations retained |
| `governance voting-history` | `/governance/voting-history` absent from the node | Hidden; explicit not-wired result before I/O |
| All eight `bridge` operations | Bridges unavailable on this testnet by owner ruling | Handled by separate PR #3406 |

The backend **does** serve `POST /api/weather/submit` in
`server/enhanced_unified_api_optimized.ts`; this tranche does not claim weather
submission is absent from WRFCoin. These CLI commands are held until they have
a separate backend integration with the required submission authentication.
NFT create/submit/verify payloads are computational-workflow requests, distinct
from the node's gated operator mutation routes; they are not repointed to an
incompatible payload or an on-chain capability the CLI cannot submit.

`service_target_integration::route_gaps_3247` runs the actual CLI binary, checks
hidden help entries and fifteen unsupported invocations with zero HTTP
expectations, and pins the supported NFT-list request and flat node response.
Four batch invocation variants additionally refuse with zero GET/POST, including
missing input files and stdin. Station registration `--batch` is independent.
The old success mocks for NFT categories, weather history and legacy batches are retired from
`command_group_integration`; their coverage is now explicit refusal coverage.

Remaining r20 coordination: wallet-watch and top-level bridge hiding intersect
open #3129; forecast intersects open #3150. DEX/bridge command hiding/refusal is the
separate PR #3406. These entries are follow-ups, not completion claims here.
