# MiningReward

## Properties

| Name            | Type                                                      | Description              | Notes      |
| --------------- | --------------------------------------------------------- | ------------------------ | ---------- |
| **id**          | **String**                                                | Unique reward identifier | [optional] |
| **recipient**   | **String**                                                | Recipient wallet address | [optional] |
| **type**        | [**TypeEnum**](#TypeEnum)                                 | Type of mining reward    | [optional] |
| **amount**      | **BigDecimal**                                            | Reward amount            | [optional] |
| **currency**    | **String**                                                | Currency symbol          | [optional] |
| **source**      | [**MiningRewardSource**](MiningRewardSource.md)           |                          | [optional] |
| **multipliers** | [**MiningRewardMultipliers**](MiningRewardMultipliers.md) |                          | [optional] |
| **performance** | [**MiningRewardPerformance**](MiningRewardPerformance.md) |                          | [optional] |
| **blockchain**  | [**MiningRewardBlockchain**](MiningRewardBlockchain.md)   |                          | [optional] |
| **timestamp**   | **OffsetDateTime**                                        |                          | [optional] |
| **status**      | [**StatusEnum**](#StatusEnum)                             |                          | [optional] |

## Enum: TypeEnum

| Name         | Value                    |
| ------------ | ------------------------ |
| WEATHER_DATA | &quot;weather_data&quot; |
| COMPUTE      | &quot;compute&quot;      |
| VALIDATION   | &quot;validation&quot;   |
| STAKING      | &quot;staking&quot;      |

## Enum: StatusEnum

| Name      | Value                 |
| --------- | --------------------- |
| PENDING   | &quot;pending&quot;   |
| CONFIRMED | &quot;confirmed&quot; |
| FAILED    | &quot;failed&quot;    |
