# HealthStatus

## Properties

| Name            | Type                                                                      | Description | Notes      |
| --------------- | ------------------------------------------------------------------------- | ----------- | ---------- |
| **status**      | [**StatusEnum**](#StatusEnum)                                             |             | [optional] |
| **timestamp**   | **OffsetDateTime**                                                        |             | [optional] |
| **uptime**      | **BigDecimal**                                                            |             | [optional] |
| **version**     | **String**                                                                |             | [optional] |
| **environment** | **String**                                                                |             | [optional] |
| **services**    | [**List&lt;HealthStatusServicesInner&gt;**](HealthStatusServicesInner.md) |             | [optional] |
| **database**    | [**HealthStatusDatabase**](HealthStatusDatabase.md)                       |             | [optional] |
| **blockchain**  | [**HealthStatusBlockchain**](HealthStatusBlockchain.md)                   |             | [optional] |
| **performance** | [**HealthStatusPerformance**](HealthStatusPerformance.md)                 |             | [optional] |
| **security**    | [**HealthStatusSecurity**](HealthStatusSecurity.md)                       |             | [optional] |

## Enum: StatusEnum

| Name      | Value                 |
| --------- | --------------------- |
| HEALTHY   | &quot;healthy&quot;   |
| DEGRADED  | &quot;degraded&quot;  |
| UNHEALTHY | &quot;unhealthy&quot; |
