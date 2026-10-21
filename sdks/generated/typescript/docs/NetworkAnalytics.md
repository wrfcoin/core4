# NetworkAnalytics

## Properties

| Name            | Type                                                              | Description | Notes                             |
| --------------- | ----------------------------------------------------------------- | ----------- | --------------------------------- |
| **overview**    | [**NetworkAnalyticsOverview**](NetworkAnalyticsOverview.md)       |             | [optional] [default to undefined] |
| **performance** | [**NetworkAnalyticsPerformance**](NetworkAnalyticsPerformance.md) |             | [optional] [default to undefined] |
| **security**    | [**NetworkAnalyticsSecurity**](NetworkAnalyticsSecurity.md)       |             | [optional] [default to undefined] |
| **crossChain**  | [**NetworkAnalyticsCrossChain**](NetworkAnalyticsCrossChain.md)   |             | [optional] [default to undefined] |
| **weather**     | [**NetworkAnalyticsWeather**](NetworkAnalyticsWeather.md)         |             | [optional] [default to undefined] |

## Example

```typescript
import { NetworkAnalytics } from '@wrfcoin/sdk-typescript';

const instance: NetworkAnalytics = {
  overview,
  performance,
  security,
  crossChain,
  weather,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
