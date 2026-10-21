# WeatherSecurityStatus

## Properties

| Name                      | Type                                                                              | Description | Notes      |
| ------------------------- | --------------------------------------------------------------------------------- | ----------- | ---------- |
| **stationSecurity**       | [**StationSecurityEnum**](#StationSecurityEnum)                                   |             | [optional] |
| **dataIntegrity**         | **BigDecimal**                                                                    |             | [optional] |
| **transmissionEncrypted** | **Boolean**                                                                       |             | [optional] |
| **lastSecurityCheck**     | **OffsetDateTime**                                                                |             | [optional] |
| **activeStations**        | **Integer**                                                                       |             | [optional] |
| **secureStations**        | **Integer**                                                                       |             | [optional] |
| **flaggedStations**       | **Integer**                                                                       |             | [optional] |
| **authenticationStatus**  | **String**                                                                        |             | [optional] |
| **apiSecurity**           | [**WeatherSecurityStatusApiSecurity**](WeatherSecurityStatusApiSecurity.md)       |             | [optional] |
| **dataValidation**        | [**WeatherSecurityStatusDataValidation**](WeatherSecurityStatusDataValidation.md) |             | [optional] |

## Enum: StationSecurityEnum

| Name        | Value                   |
| ----------- | ----------------------- |
| SECURE      | &quot;secure&quot;      |
| WARNING     | &quot;warning&quot;     |
| COMPROMISED | &quot;compromised&quot; |
