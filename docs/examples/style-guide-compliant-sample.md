# Quick Start: Weather Data API Integration

Get started with the WRFCoin platform weather data API in under 10 minutes.

## Overview

The WRFCoin platform weather oracle provides real-time and historical weather data through a RESTful
API. You can integrate this data into your applications for computational NFT creation, cross-chain
bridge operations, or DAO governance decisions.

## Prerequisites

Before you begin, ensure you have:

- [ ] A WRFCoin platform API key ([get one here](../api/authentication.md))
- [ ] Node.js 18+ installed on your system
- [ ] Basic familiarity with REST APIs
- [ ] 5 minutes of your time

**Estimated Time**: 8 minutes **Difficulty**: Beginner

## Step 1: Install the SDK

Install the official WRFCoin platform SDK using npm:

```bash
npm install @wrfcoin/sdk
```

**Expected Output:**

```
+ @wrfcoin/sdk@1.2.3
added 15 packages from 8 contributors
```

## Step 2: Configure Authentication

Create a configuration file with your API credentials:

```typescript
import { WRFCoinSDK } from '@wrfcoin/sdk';

const sdk = new WRFCoinSDK({
  apiKey: process.env.WRFCOIN_API_KEY,
  environment: 'production',
  enableRetries: true,
});
```

**Expected Output:** Your SDK instance is now configured and ready to make API calls.

## Step 3: Fetch Weather Data

Request current weather data for a specific location:

```typescript
async function getCurrentWeather() {
  try {
    const weatherData = await sdk.weather.getCurrentData({
      latitude: 35.6762,
      longitude: 139.6503,
      includeMetrics: ['temperature', 'humidity', 'pressure'],
    });

    console.log('Current weather:', weatherData);
    return weatherData;
  } catch (error) {
    console.error('Error fetching weather data:', error.message);
    throw error;
  }
}
```

**Expected Output:**

```json
{
  "location": {
    "latitude": 35.6762,
    "longitude": 139.6503,
    "city": "Tokyo"
  },
  "current": {
    "temperature": 22.5,
    "humidity": 65,
    "pressure": 1013.2
  },
  "timestamp": "2025-01-22T10:30:00Z"
}
```

## Verify Your Setup

To confirm everything works correctly:

1. Run your weather data script
2. Check that you receive a JSON response with weather data
3. Verify the timestamp is recent (within the last few minutes)

If you see the weather data response above, you're all set!

## Troubleshooting

**Issue**: "API key is invalid" error **Solution**: Verify your API key in the
[developer portal](../developer-portal.md) and ensure it's correctly set in your environment
variables.

**Issue**: "Rate limit exceeded" error  
**Solution**: Wait 60 seconds and retry. Consider upgrading to a higher tier for increased rate
limits.

## Next Steps

Now that you have weather data integration working, you can:

- [Create computational NFTs](../tutorials/computational-nfts.md) - Learn to generate weather-based
  NFTs
- [Build weather applications](../examples/weather-dashboard.md) - See complete application examples
- [Explore advanced features](../api/weather-oracle.md) - Access historical data and forecasts
- [Join our community](https://discord.gg/wrfcoin) - Get help and share your projects

## Related Resources

- [Weather Oracle API Reference](../api/weather-oracle.md)
- [SDK Documentation](../sdk/README.md)
- [Example Applications](../examples/complete-applications/)

---

**Need Help?** Join our [Discord community](https://discord.gg/wrfcoin) or
[open an issue](https://github.com/wrfcoin/platform/issues).
