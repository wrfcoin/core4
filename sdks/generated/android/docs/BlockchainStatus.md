# BlockchainStatus

## Properties

| Name                | Type                                          | Description                   | Notes      |
| ------------------- | --------------------------------------------- | ----------------------------- | ---------- |
| **blockHeight**     | **Integer**                                   | Current block height          | [optional] |
| **lastBlockHash**   | **String**                                    | Hash of the most recent block | [optional] |
| **networkHashRate** | **BigDecimal**                                | Network hash rate in H/s      | [optional] |
| **difficulty**      | **BigDecimal**                                | Current mining difficulty     | [optional] |
| **consensusState**  | [**ConsensusStateEnum**](#ConsensusStateEnum) |                               | [optional] |
| **peerCount**       | **Integer**                                   | Number of connected peers     | [optional] |
| **syncStatus**      | [**SyncStatusEnum**](#SyncStatusEnum)         |                               | [optional] |
| **uptimeSeconds**   | **Integer**                                   | Node uptime in seconds        | [optional] |

## Enum: ConsensusStateEnum

| Name     | Value                |
| -------- | -------------------- |
| HEALTHY  | &quot;healthy&quot;  |
| DEGRADED | &quot;degraded&quot; |
| FAILED   | &quot;failed&quot;   |

## Enum: SyncStatusEnum

| Name        | Value                   |
| ----------- | ----------------------- |
| SYNCED      | &quot;synced&quot;      |
| SYNCING     | &quot;syncing&quot;     |
| OUT_OF_SYNC | &quot;out_of_sync&quot; |
