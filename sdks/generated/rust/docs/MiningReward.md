# MiningReward

## Properties

| Name            | Type                                                                       | Description              | Notes      |
| --------------- | -------------------------------------------------------------------------- | ------------------------ | ---------- |
| **id**          | Option<**String**>                                                         | Unique reward identifier | [optional] |
| **recipient**   | Option<**String**>                                                         | Recipient wallet address | [optional] |
| **r#type**      | Option<**String**>                                                         | Type of mining reward    | [optional] |
| **amount**      | Option<**f64**>                                                            | Reward amount            | [optional] |
| **currency**    | Option<**String**>                                                         | Currency symbol          | [optional] |
| **source**      | Option<[**models::MiningRewardSource**](MiningReward_source.md)>           |                          | [optional] |
| **multipliers** | Option<[**models::MiningRewardMultipliers**](MiningReward_multipliers.md)> |                          | [optional] |
| **performance** | Option<[**models::MiningRewardPerformance**](MiningReward_performance.md)> |                          | [optional] |
| **blockchain**  | Option<[**models::MiningRewardBlockchain**](MiningReward_blockchain.md)>   |                          | [optional] |
| **timestamp**   | Option<**String**>                                                         |                          | [optional] |
| **status**      | Option<**String**>                                                         |                          | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
