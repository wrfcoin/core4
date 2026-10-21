#!/bin/bash
# Example: Interledger Payment Request (IPR) Integration with Wrfcoin CLI
# This script demonstrates how to implement IPR for paid API access to weather forecast data

# Step 1: Create a dedicated wallet for IPR transactions
echo "Creating wallet for IPR transactions..."
IPR_WALLET_ADDRESS=$(wrfcoin wallet new-address --name "IPR Weather API Wallet" --json | jq -r '.address')
echo "IPR wallet address: $IPR_WALLET_ADDRESS"

# Step 2: Set up Interledger payment server for the Weather API
echo "Setting up Interledger Payment Request server..."
wrfcoin crosschain setup-ilp-payment-server \
  --server-port 7770 \
  --wallet-address "$IPR_WALLET_ADDRESS" \
  --database-path "./ilp_payment_data.db" \
  --enable-cors \
  --tls-cert "./certs/server.crt" \
  --tls-key "./certs/server.key"

# Step 3: Create pricing tiers for the Weather API
cat > pricing_tiers.json << EOF
{
  "tiers": [
    {
      "name": "basic",
      "description": "Basic weather forecast access - 3-day, low resolution",
      "price": 0.001,
      "currency": "WRF",
      "rate_limit": 10,
      "time_period": "hour"
    },
    {
      "name": "premium",
      "description": "Premium weather forecast access - 7-day, high resolution",
      "price": 0.01,
      "currency": "WRF",
      "rate_limit": 100,
      "time_period": "hour"
    },
    {
      "name": "enterprise",
      "description": "Enterprise weather forecast access - 14-day, all data variables",
      "price": 0.1,
      "currency": "WRF",
      "rate_limit": 1000,
      "time_period": "hour"
    }
  ],
  "payment_methods": ["interledger"],
  "expiration_policy": {
    "basic": "24h",
    "premium": "7d",
    "enterprise": "30d"
  }
}
EOF

# Step 4: Register API pricing with the payment server
echo "Registering API pricing tiers..."
wrfcoin crosschain ilp-register-api \
  --name "WeatherForecastAPI" \
  --description "Wrfcoin Weather Forecast API with pay-per-use access" \
  --pricing-file pricing_tiers.json \
  --payment-pointer-prefix "https://api.wrfcoin.network/pay" \
  --wallet-address "$IPR_WALLET_ADDRESS"

# Step 5: Generate IPR payment pointers for different services
echo "Generating IPR payment pointers..."

# For basic tier
BASIC_POINTER=$(wrfcoin crosschain ilp-generate-pointer \
  --api-name "WeatherForecastAPI" \
  --tier "basic" \
  --json | jq -r '.paymentPointer')
echo "Basic tier payment pointer: $BASIC_POINTER"

# For premium tier
PREMIUM_POINTER=$(wrfcoin crosschain ilp-generate-pointer \
  --api-name "WeatherForecastAPI" \
  --tier "premium" \
  --json | jq -r '.paymentPointer')
echo "Premium tier payment pointer: $PREMIUM_POINTER"

# For enterprise tier
ENTERPRISE_POINTER=$(wrfcoin crosschain ilp-generate-pointer \
  --api-name "WeatherForecastAPI" \
  --tier "enterprise" \
  --json | jq -r '.paymentPointer')
echo "Enterprise tier payment pointer: $ENTERPRISE_POINTER"

# Step 6: Create a sample weather API client configuration
cat > client_config.json << EOF
{
  "api_url": "https://api.wrfcoin.network/forecast",
  "payment_pointer": "$PREMIUM_POINTER",
  "client_id": "example_client",
  "wallet_address": "$IPR_WALLET_ADDRESS",
  "max_payment": 1.0,
  "auto_refresh": true,
  "refresh_threshold": 0.2
}
EOF

# Step 7: Create a server that handles IPR payments for forecast data
cat > server_example.js << EOF
// Sample Node.js code for the payment server handler
const express = require('express');
const ilp = require('ilp');
const app = express();
const port = 3000;

// Configure Interledger payment handler middleware
app.use('/forecast', ilp.createMiddleware({
  paymentPointerMap: {
    '/basic': '$BASIC_POINTER',
    '/premium': '$PREMIUM_POINTER',
    '/enterprise': '$ENTERPRISE_POINTER'
  },
  authTokens: {},
  onPayment: (req, res, next, amount) => {
    console.log(\`Received payment of \${amount} for \${req.path}\`);
    // Store payment information in the database
    // Set authorization for this client
    next();
  }
}));

// API routes with payment requirements
app.get('/forecast/basic/:location', (req, res) => {
  // Return basic forecast for the location
  res.json({
    location: req.params.location,
    forecast: "3-day basic forecast data",
    resolution: "low",
    paid: true
  });
});

app.get('/forecast/premium/:location', (req, res) => {
  // Return premium forecast for the location
  res.json({
    location: req.params.location,
    forecast: "7-day premium forecast data",
    resolution: "high",
    variables: ["temperature", "humidity", "pressure", "wind", "precipitation"],
    paid: true
  });
});

app.get('/forecast/enterprise/:location', (req, res) => {
  // Return enterprise forecast for the location
  res.json({
    location: req.params.location,
    forecast: "14-day enterprise forecast data",
    resolution: "very-high",
    variables: ["temperature", "humidity", "pressure", "wind", "precipitation", 
                "cloud_cover", "solar_radiation", "soil_moisture", "visibility"],
    ensembles: true,
    uncertainty: "quantified",
    paid: true
  });
});

app.listen(port, () => {
  console.log(\`Forecast API with Interledger payment support listening on port \${port}\`);
});
EOF

# Step 8: Simulate a client accessing the paid API
echo "Simulating client accessing the paid API..."
wrfcoin crosschain ilp-pay-api \
  --config-file client_config.json \
  --api-path "/forecast/premium/newyork" \
  --output-file "premium_forecast.json"

# Step 9: Query payment history for the API
echo "Querying payment history..."
wrfcoin crosschain ilp-payment-history \
  --wallet-address "$IPR_WALLET_ADDRESS" \
  --api-name "WeatherForecastAPI" \
  --days 7 \
  --json | jq '.'

# Step 10: Set up automated payment channels for data subscribers
echo "Setting up automated payment channel for data subscription..."
wrfcoin crosschain ilp-setup-payment-channel \
  --source-wallet "$IPR_WALLET_ADDRESS" \
  --destination-pointer "$PREMIUM_POINTER" \
  --channel-amount 0.5 \
  --packet-amount 0.001 \
  --interval-ms 3600000 \
  --auto-refill true \
  --refill-threshold 0.1 \
  --max-refill-amount 0.5

# Step 11: Create weather data subscription using payment channel
echo "Creating weather data subscription..."
wrfcoin weather subscribe \
  --tier premium \
  --region "North America" \
  --parameters "temperature,precipitation,wind" \
  --update-frequency "hourly" \
  --duration "30d" \
  --payment-method interledger \
  --payment-pointer "$PREMIUM_POINTER" \
  --wallet-address "$IPR_WALLET_ADDRESS"

# Step 12: Set up access control based on payment
echo "Setting up payment-based access control for weather API..."
wrfcoin crosschain ilp-configure-access-control \
  --api-name "WeatherForecastAPI" \
  --tier-permissions-file pricing_tiers.json \
  --auth-method "payment-token" \
  --token-validity "24h" \
  --enable-refunds \
  --refund-policy "prorated"

# Step 13: Generate API client library with Interledger support
echo "Generating API client library with Interledger support..."
wrfcoin crosschain ilp-generate-client \
  --api-name "WeatherForecastAPI" \
  --language "javascript" \
  --output-dir "./weather-api-client" \
  --include-examples true \
  --include-ilp-payment true

# Step 14: Monitor IPR server and payment status
echo "Setting up monitoring for 30 seconds..."
timeout 30s wrfcoin monitor ilp-server \
  --interval 5 \
  --server-port 7770 \
  --format text

echo "Interledger Payment Request integration demonstration complete!"