# HealthStatus

## Properties

| Name            | Type                                                                                 | Description | Notes      |
| --------------- | ------------------------------------------------------------------------------------ | ----------- | ---------- |
| **status**      | Option<**String**>                                                                   |             | [optional] |
| **timestamp**   | Option<**String**>                                                                   |             | [optional] |
| **uptime**      | Option<**f64**>                                                                      |             | [optional] |
| **version**     | Option<**String**>                                                                   |             | [optional] |
| **environment** | Option<**String**>                                                                   |             | [optional] |
| **services**    | Option<[**Vec<models::HealthStatusServicesInner>**](HealthStatus_services_inner.md)> |             | [optional] |
| **database**    | Option<[**models::HealthStatusDatabase**](HealthStatus_database.md)>                 |             | [optional] |
| **blockchain**  | Option<[**models::HealthStatusBlockchain**](HealthStatus_blockchain.md)>             |             | [optional] |
| **performance** | Option<[**models::HealthStatusPerformance**](HealthStatus_performance.md)>           |             | [optional] |
| **security**    | Option<[**models::HealthStatusSecurity**](HealthStatus_security.md)>                 |             | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
