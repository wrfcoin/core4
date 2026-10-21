# ApiComputeProvidersGet200ResponseProvidersInner

## Properties

| Name             | Type                                                                                                                        | Description | Notes      |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **name**         | **String**                                                                                                                  |             | [optional] |
| **status**       | [**StatusEnum**](#StatusEnum)                                                                                               |             | [optional] |
| **availability** | **BigDecimal**                                                                                                              |             | [optional] |
| **pricePerHour** | **String**                                                                                                                  |             | [optional] |
| **capabilities** | **List&lt;String&gt;**                                                                                                      |             | [optional] |
| **resources**    | [**ApiComputeProvidersGet200ResponseProvidersInnerResources**](ApiComputeProvidersGet200ResponseProvidersInnerResources.md) |             | [optional] |

## Enum: StatusEnum

| Name        | Value                   |
| ----------- | ----------------------- |
| ACTIVE      | &quot;active&quot;      |
| INACTIVE    | &quot;inactive&quot;    |
| MAINTENANCE | &quot;maintenance&quot; |
