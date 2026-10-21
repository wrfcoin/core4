/**
 * K6 Prometheus Integration Configuration
 * Sends K6 metrics to Prometheus for monitoring and alerting
 */

import http from 'k6/http';
import { Counter, Rate, Trend, Gauge } from 'k6/metrics';

// Custom K6 metrics for Prometheus
export const k6Metrics = {
  // Performance metrics
  httpRequestDuration: new Trend('k6_http_request_duration_ms'),
  httpRequestsTotal: new Counter('k6_http_requests_total'),
  httpRequestsFailed: new Counter('k6_http_requests_failed_total'),

  // Load testing specific metrics
  virtualUsers: new Gauge('k6_virtual_users'),
  iterations: new Counter('k6_iterations_total'),
  checksFailedRate: new Rate('k6_checks_failed_rate'),

  // WRFCoin specific metrics
  sensorPostsRate: new Rate('k6_sensor_posts_per_minute'),
  databaseQueryTime: new Trend('k6_database_query_duration_ms'),
  cacheHitRate: new Rate('k6_cache_hit_rate'),
  serviceHealthScore: new Gauge('k6_service_health_score'),

  // System resource metrics
  systemCpuUsage: new Gauge('k6_system_cpu_usage_percent'),
  systemMemoryUsage: new Gauge('k6_system_memory_usage_percent'),
  connectionPoolUsage: new Gauge('k6_connection_pool_usage_percent'),

  // Business metrics
  transactionSuccessRate: new Rate('k6_transaction_success_rate'),
  carbonCreditProcessingTime: new Trend('k6_carbon_credit_processing_ms'),
  bridgeOperationTime: new Trend('k6_bridge_operation_duration_ms'),
};

/**
 * Prometheus Pushgateway integration
 */
export class PrometheusIntegration {
  constructor(pushgatewayUrl = 'http://localhost:9091') {
    this.pushgatewayUrl = pushgatewayUrl;
    this.jobName = 'k6_load_test';
    this.instanceId = `k6_${Date.now()}`;
    this.labels = {
      job: this.jobName,
      instance: this.instanceId,
      environment: __ENV.ENVIRONMENT || 'development',
      test_scenario: __ENV.K6_SCENARIO || 'unknown',
    };
  }

  /**
   * Push metrics to Prometheus Pushgateway
   */
  pushMetrics(metrics) {
    const prometheusData = this.formatMetricsForPrometheus(metrics);

    const response = http.post(
      `${this.pushgatewayUrl}/metrics/job/${this.jobName}/instance/${this.instanceId}`,
      prometheusData,
      {
        headers: {
          'Content-Type': 'text/plain; version=0.0.4; charset=utf-8',
        },
        timeout: '10s',
      }
    );

    return response.status === 200;
  }

  /**
   * Format K6 metrics for Prometheus
   */
  formatMetricsForPrometheus(metrics) {
    let prometheusText = '';

    for (const [metricName, metricData] of Object.entries(metrics)) {
      const labels = this.formatLabels(metricData.tags || {});

      // Handle different metric types
      switch (metricData.type) {
        case 'counter':
          prometheusText += `# TYPE ${metricName} counter\n`;
          prometheusText += `${metricName}${labels} ${metricData.value}\n`;
          break;

        case 'gauge':
          prometheusText += `# TYPE ${metricName} gauge\n`;
          prometheusText += `${metricName}${labels} ${metricData.value}\n`;
          break;

        case 'histogram':
          prometheusText += `# TYPE ${metricName} histogram\n`;
          if (metricData.buckets) {
            for (const [bucket, count] of Object.entries(metricData.buckets)) {
              prometheusText += `${metricName}_bucket${this.formatLabels({ ...metricData.tags, le: bucket })} ${count}\n`;
            }
          }
          prometheusText += `${metricName}_sum${labels} ${metricData.sum || 0}\n`;
          prometheusText += `${metricName}_count${labels} ${metricData.count || 0}\n`;
          break;

        default:
          // Default to gauge
          prometheusText += `# TYPE ${metricName} gauge\n`;
          prometheusText += `${metricName}${labels} ${metricData.value || 0}\n`;
      }
    }

    return prometheusText;
  }

  /**
   * Format labels for Prometheus
   */
  formatLabels(tags = {}) {
    const allLabels = { ...this.labels, ...tags };
    const labelPairs = Object.entries(allLabels)
      .map(([key, value]) => `${key}="${value}"`)
      .join(',');

    return labelPairs ? `{${labelPairs}}` : '';
  }

  /**
   * Record K6 metric in Prometheus format
   */
  recordMetric(name, value, type = 'gauge', tags = {}) {
    const metricData = {
      value,
      type,
      tags,
      timestamp: Date.now(),
    };

    this.pushMetrics({ [name]: metricData });
  }
}

/**
 * InfluxDB integration for time-series data
 */
export class InfluxDBIntegration {
  constructor(config = {}) {
    this.url = config.url || 'http://localhost:8086';
    this.database = config.database || 'k6_metrics';
    this.username = config.username || 'k6';
    this.password = config.password || 'k6password';
    this.measurement = 'k6_metrics';
  }

  /**
   * Write metrics to InfluxDB
   */
  writeMetrics(metrics) {
    const influxData = this.formatMetricsForInflux(metrics);

    const response = http.post(
      `${this.url}/write?db=${this.database}`,
      influxData,
      {
        headers: {
          'Content-Type': 'application/octet-stream',
          Authorization: this.getAuthHeader(),
        },
        timeout: '10s',
      }
    );

    return response.status === 204;
  }

  /**
   * Format metrics for InfluxDB line protocol
   */
  formatMetricsForInflux(metrics) {
    let influxLines = [];

    for (const [metricName, metricData] of Object.entries(metrics)) {
      const tags = this.formatInfluxTags(metricData.tags || {});
      const fields = this.formatInfluxFields(metricData);
      const timestamp = metricData.timestamp || Date.now() * 1000000; // nanoseconds

      influxLines.push(`${this.measurement},${tags} ${fields} ${timestamp}`);
    }

    return influxLines.join('\n');
  }

  formatInfluxTags(tags) {
    const baseTags = {
      job: 'k6_load_test',
      environment: __ENV.ENVIRONMENT || 'development',
      scenario: __ENV.K6_SCENARIO || 'unknown',
    };

    const allTags = { ...baseTags, ...tags };
    return Object.entries(allTags)
      .map(([key, value]) => `${key}=${value}`)
      .join(',');
  }

  formatInfluxFields(metricData) {
    const fields = [`value=${metricData.value}`];

    if (metricData.min !== undefined) fields.push(`min=${metricData.min}`);
    if (metricData.max !== undefined) fields.push(`max=${metricData.max}`);
    if (metricData.avg !== undefined) fields.push(`avg=${metricData.avg}`);
    if (metricData.p95 !== undefined) fields.push(`p95=${metricData.p95}`);
    if (metricData.p99 !== undefined) fields.push(`p99=${metricData.p99}`);

    return fields.join(',');
  }

  getAuthHeader() {
    if (this.username && this.password) {
      const credentials = btoa(`${this.username}:${this.password}`);
      return `Basic ${credentials}`;
    }
    return '';
  }
}

/**
 * Grafana Dashboard Integration
 */
export class GrafanaDashboardIntegration {
  constructor(grafanaUrl = 'http://localhost:3004') {
    this.grafanaUrl = grafanaUrl;
    this.apiKey = __ENV.GRAFANA_API_KEY;
  }

  /**
   * Create K6 load testing dashboard
   */
  async createLoadTestDashboard() {
    const dashboard = this.buildLoadTestDashboard();

    const response = http.post(
      `${this.grafanaUrl}/api/dashboards/db`,
      JSON.stringify({ dashboard, overwrite: true }),
      {
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${this.apiKey}`,
        },
      }
    );

    return response.status === 200;
  }

  buildLoadTestDashboard() {
    return {
      id: null,
      title: 'WRFCoin K6 Load Testing Dashboard',
      tags: ['k6', 'load-testing', 'wrfcoin'],
      timezone: 'browser',
      panels: [
        {
          id: 1,
          title: 'HTTP Request Duration',
          type: 'graph',
          targets: [
            {
              expr: 'k6_http_request_duration_ms',
              legendFormat: 'Request Duration',
            },
          ],
          yAxes: [{ label: 'Duration (ms)' }, { show: false }],
        },
        {
          id: 2,
          title: 'Virtual Users',
          type: 'graph',
          targets: [
            {
              expr: 'k6_virtual_users',
              legendFormat: 'Active VUs',
            },
          ],
        },
        {
          id: 3,
          title: 'Sensor Posts Rate',
          type: 'singlestat',
          targets: [
            {
              expr: 'rate(k6_sensor_posts_per_minute[5m])',
              legendFormat: 'Posts/min',
            },
          ],
        },
        {
          id: 4,
          title: 'Error Rate',
          type: 'graph',
          targets: [
            {
              expr: 'rate(k6_http_requests_failed_total[5m]) / rate(k6_http_requests_total[5m])',
              legendFormat: 'Error Rate %',
            },
          ],
        },
        {
          id: 5,
          title: 'Database Performance',
          type: 'graph',
          targets: [
            {
              expr: 'k6_database_query_duration_ms',
              legendFormat: 'DB Query Time',
            },
          ],
        },
        {
          id: 6,
          title: 'Service Health Score',
          type: 'gauge',
          targets: [
            {
              expr: 'k6_service_health_score',
              legendFormat: 'Health Score',
            },
          ],
        },
      ],
      time: {
        from: 'now-1h',
        to: 'now',
      },
      refresh: '5s',
    };
  }
}

/**
 * Alerting Integration
 */
export class AlertingIntegration {
  constructor() {
    this.thresholds = new Map();
    this.alertsTriggered = new Set();
  }

  /**
   * Define alert thresholds
   */
  setThresholds() {
    this.thresholds.set('high_error_rate', {
      metric: 'k6_checks_failed_rate',
      condition: 'greater_than',
      value: 0.05, // 5% error rate
      severity: 'warning',
    });

    this.thresholds.set('high_response_time', {
      metric: 'k6_http_request_duration_ms',
      condition: 'greater_than',
      value: 2000, // 2 second response time
      severity: 'warning',
    });

    this.thresholds.set('system_overload', {
      metric: 'k6_system_cpu_usage_percent',
      condition: 'greater_than',
      value: 90, // 90% CPU usage
      severity: 'critical',
    });
  }

  /**
   * Check thresholds and trigger alerts
   */
  checkThresholds(metrics) {
    for (const [alertName, threshold] of this.thresholds) {
      const metricValue = metrics[threshold.metric];

      if (
        this.evaluateCondition(
          metricValue,
          threshold.condition,
          threshold.value
        )
      ) {
        if (!this.alertsTriggered.has(alertName)) {
          this.triggerAlert(alertName, threshold, metricValue);
          this.alertsTriggered.add(alertName);
        }
      } else {
        // Clear alert if condition no longer met
        if (this.alertsTriggered.has(alertName)) {
          this.clearAlert(alertName);
          this.alertsTriggered.delete(alertName);
        }
      }
    }
  }

  evaluateCondition(value, condition, threshold) {
    switch (condition) {
      case 'greater_than':
        return value > threshold;
      case 'less_than':
        return value < threshold;
      case 'equals':
        return value === threshold;
      default:
        return false;
    }
  }

  triggerAlert(alertName, threshold, value) {
    console.warn(
      `🚨 ALERT: ${alertName} - ${threshold.metric} = ${value} (threshold: ${threshold.value})`
    );

    // Send alert to monitoring system
    this.sendAlert({
      name: alertName,
      metric: threshold.metric,
      value: value,
      threshold: threshold.value,
      severity: threshold.severity,
      timestamp: Date.now(),
    });
  }

  clearAlert(alertName) {
    console.log(`✅ ALERT CLEARED: ${alertName}`);
  }

  sendAlert(alertData) {
    // Send to Alertmanager or other alerting system
    const alertmanagerUrl = __ENV.ALERTMANAGER_URL || 'http://localhost:9093';

    http.post(
      `${alertmanagerUrl}/api/v1/alerts`,
      JSON.stringify([
        {
          labels: {
            alertname: alertData.name,
            severity: alertData.severity,
            service: 'k6_load_test',
          },
          annotations: {
            summary: `Load test alert: ${alertData.name}`,
            description: `Metric ${alertData.metric} exceeded threshold: ${alertData.value} > ${alertData.threshold}`,
          },
          startsAt: new Date(alertData.timestamp).toISOString(),
        },
      ]),
      {
        headers: { 'Content-Type': 'application/json' },
      }
    );
  }
}

/**
 * Monitoring Integration Manager
 */
export class MonitoringManager {
  constructor(config = {}) {
    this.prometheus = new PrometheusIntegration(config.prometheusUrl);
    this.influxdb = new InfluxDBIntegration(config.influxdb);
    this.grafana = new GrafanaDashboardIntegration(config.grafanaUrl);
    this.alerting = new AlertingIntegration();

    this.enabled = __ENV.MONITORING_ENABLED === 'true';
    this.alerting.setThresholds();
  }

  /**
   * Initialize monitoring integrations
   */
  async initialize() {
    if (!this.enabled) {
      console.log('📊 Monitoring integration disabled');
      return;
    }

    console.log('📊 Initializing monitoring integrations...');

    // Create Grafana dashboard
    try {
      await this.grafana.createLoadTestDashboard();
      console.log('✅ Grafana dashboard created');
    } catch (error) {
      console.warn('⚠️ Failed to create Grafana dashboard:', error.message);
    }
  }

  /**
   * Send metrics to all configured monitoring systems
   */
  sendMetrics(metrics) {
    if (!this.enabled) return;

    // Send to Prometheus
    if (__ENV.K6_PROMETHEUS_ENABLED === 'true') {
      this.prometheus.pushMetrics(metrics);
    }

    // Send to InfluxDB
    if (__ENV.K6_INFLUXDB_ENABLED === 'true') {
      this.influxdb.writeMetrics(metrics);
    }

    // Check alert thresholds
    this.alerting.checkThresholds(metrics);
  }

  /**
   * Record custom metric
   */
  recordMetric(name, value, tags = {}) {
    if (!this.enabled) return;

    const metricData = {
      value,
      type: 'gauge',
      tags,
      timestamp: Date.now(),
    };

    this.sendMetrics({ [name]: metricData });
  }
}

// Export monitoring components
export default {
  k6Metrics,
  PrometheusIntegration,
  InfluxDBIntegration,
  GrafanaDashboardIntegration,
  AlertingIntegration,
  MonitoringManager,
};
