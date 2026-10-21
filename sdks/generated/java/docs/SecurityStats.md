# SecurityStats

## Properties

| Name                  | Type                                    | Description | Notes      |
| --------------------- | --------------------------------------- | ----------- | ---------- |
| **totalEvents**       | **Integer**                             |             | [optional] |
| **threatLevel**       | [**ThreatLevelEnum**](#ThreatLevelEnum) |             | [optional] |
| **activeThreatCount** | **Integer**                             |             | [optional] |
| **blockedAttacks24h** | **Integer**                             |             | [optional] |
| **securityScore**     | **BigDecimal**                          |             | [optional] |
| **lastUpdated**       | **OffsetDateTime**                      |             | [optional] |

## Enum: ThreatLevelEnum

| Name     | Value                |
| -------- | -------------------- |
| LOW      | &quot;low&quot;      |
| MEDIUM   | &quot;medium&quot;   |
| HIGH     | &quot;high&quot;     |
| CRITICAL | &quot;critical&quot; |
