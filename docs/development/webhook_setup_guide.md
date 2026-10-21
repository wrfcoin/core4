# Wrfcoin Monitoring Webhook Setup Guide

## Current Status ✅

Your blockchain monitoring dashboard webhook system is **properly configured and running**! Here's
what's currently set up:

### ✅ What's Working

- **Webhook Handler Service**: Running at `http://localhost:5001`
- **AlertManager**: Configured with comprehensive alert routing
- **Email Integration**: Ready (needs SMTP configuration)
- **Slack Integration**: Ready (needs webhook URL)
- **Discord Integration**: Ready (needs webhook URL)

### 🔧 What Needs Configuration

To complete the setup, you need to add webhook URLs to your environment:

## 1. Slack Setup

### Create Slack Webhook:

1. Go to https://api.slack.com/apps
2. Create a new app or select existing app
3. Go to "Incoming Webhooks" and activate them
4. Click "Add New Webhook to Workspace"
5. Choose the channel for alerts (e.g., #blockchain-alerts)
6. Copy the webhook URL

### Configure Slack:

```bash
export SLACK_WEBHOOK_URL="https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK"
```

## 2. Discord Setup

### Create Discord Webhook:

1. Open Discord and go to your server
2. Right-click on the channel for alerts
3. Select "Edit Channel" → "Integrations" → "Webhooks"
4. Click "New Webhook"
5. Give it a name (e.g., "Wrfcoin Alerts")
6. Copy the webhook URL

### Configure Discord:

```bash
export DISCORD_WEBHOOK_URL="https://discord.com/api/webhooks/YOUR/DISCORD/WEBHOOK"
```

## 3. Email Setup

### Configure SMTP (Gmail example):

```bash
export SMTP_FROM="alerts@wrfcoin.com"
export SMTP_USERNAME="your_gmail@gmail.com"
export SMTP_PASSWORD="your_app_password"
```

### For Gmail App Password:

1. Enable 2FA on your Google account
2. Go to Google Account settings → Security → App passwords
3. Generate an app password for "Mail"
4. Use that password in SMTP_PASSWORD

## 4. Complete Environment Setup

Add to your `.env` file:

```bash
# Webhook Configuration
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK
DISCORD_WEBHOOK_URL=https://discord.com/api/webhooks/YOUR/DISCORD/WEBHOOK
WEBHOOK_SECRET=your_secure_webhook_secret

# Email Configuration
SMTP_FROM=alerts@wrfcoin.com
SMTP_USERNAME=your_email@gmail.com
SMTP_PASSWORD=your_app_password

# Grafana
GRAFANA_ADMIN_PASSWORD=secure_password
REDIS_PASSWORD=example_redis_password
```

## 5. Restart Services

After adding environment variables:

```bash
cd infrastructure/monitoring
docker-compose -f docker-compose.monitoring-complete.yml down
docker-compose -f docker-compose.monitoring-complete.yml up -d
```

## 6. Test the Setup

Run the test script:

```bash
python3 test_webhook_setup.py
```

## Alert Categories Configured

Your AlertManager is configured to route different types of alerts to appropriate channels:

### Critical Alerts → Multiple Channels

- **Email**: blockchain-oncall@wrfcoin.com
- **Slack**: #blockchain-critical
- **Webhook**: Enhanced processing

### Specialized Alert Routing

- **Consensus Alerts** → #consensus-monitoring
- **Bridge Alerts** → #cross-chain-monitoring
- **Security Alerts** → #security-alerts
- **Infrastructure Alerts** → #infrastructure-monitoring
- **API Alerts** → #api-monitoring
- **Weather/IoT Alerts** → #weather-monitoring

## Dashboard Access

- **Grafana**: http://localhost:3000 (admin/admin123)
- **Prometheus**: http://localhost:9090
- **AlertManager**: http://localhost:9093
- **Jaeger Tracing**: http://localhost:16686

## Advanced Features

### Trace Correlation

Your alerts automatically include:

- Related distributed traces
- Jaeger query links
- Performance correlation
- Service dependency mapping

### Intelligent Routing

- Severity-based escalation
- Category-specific channels
- Time-based muting
- Alert suppression rules

## Testing Alerts

Once webhooks are configured, you can test with:

```bash
# Test specific alert type
curl -X POST http://localhost:9093/api/v1/alerts \
  -H "Content-Type: application/json" \
  -d '[{
    "labels": {
      "alertname": "TestAlert",
      "severity": "warning",
      "category": "test"
    },
    "annotations": {
      "summary": "Test alert for verification"
    }
  }]'
```

## Monitoring the Monitors

The webhook handler itself exposes metrics at:

- Health: http://localhost:5001/health
- Metrics: http://localhost:5001/metrics

Your monitoring system is enterprise-ready with comprehensive alerting, trace correlation, and
multi-channel notifications! 🚀
