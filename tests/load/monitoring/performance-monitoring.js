/**
 * Performance Monitoring Tools for K6 Load Testing
 * System-level monitoring and APM integration
 */

import http from 'k6/http';
import { check, group } from 'k6';
import { Counter, Rate, Trend, Gauge } from 'k6/metrics';
import { SERVICE_REGISTRY } from '../k6.config.js';

// System Performance Metrics
const systemCpuUsage = new Gauge('system_cpu_usage_percent');
const systemMemoryUsage = new Gauge('system_memory_usage_percent');
const systemDiskUsage = new Gauge('system_disk_usage_percent');
const systemNetworkIn = new Counter('system_network_bytes_received');
const systemNetworkOut = new Counter('system_network_bytes_sent');
const systemLoadAverage = new Gauge('system_load_average');

// Application Performance Metrics
const applicationResponseTime = new Trend('application_response_time_ms');
const applicationThroughput = new Rate('application_requests_per_second');
const applicationErrorRate = new Rate('application_error_rate');
const databaseConnectionsActive = new Gauge('database_connections_active');
const databaseConnectionsIdle = new Gauge('database_connections_idle');
const cacheHitRatio = new Gauge('cache_hit_ratio_percent');

// Service-Specific Metrics
const iotServiceMetrics = {
  sensorPostsProcessed: new Counter('iot_sensor_posts_processed_total'),
  averageProcessingTime: new Trend('iot_processing_time_ms'),
  queueDepth: new Gauge('iot_queue_depth'),
  rejectedPosts: new Counter('iot_rejected_posts_total'),
};

const walletServiceMetrics = {
  transactionsProcessed: new Counter('wallet_transactions_processed_total'),
  balanceQueries: new Counter('wallet_balance_queries_total'),
  averageTransactionTime: new Trend('wallet_transaction_time_ms'),
};

const crossChainMetrics = {
  bridgeOperations: new Counter('cross_chain_bridge_operations_total'),
  bridgeLatency: new Trend('cross_chain_bridge_latency_ms'),
  failedBridges: new Counter('cross_chain_failed_bridges_total'),
};

/**
 * System Resource Monitor
 */
export class SystemResourceMonitor {
  constructor() {
    this.monitoringEndpoint = `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/monitoring`;
    this.enabled = __ENV.SYSTEM_MONITORING_ENABLED !== 'false';
    this.interval = parseInt(__ENV.MONITORING_INTERVAL || '10000'); // 10 seconds
    this.lastCollection = 0;
  }

  /**
   * Collect system metrics from monitoring endpoint
   */
  collectSystemMetrics() {
    if (!this.enabled) return;

    const now = Date.now();
    if (now - this.lastCollection < this.interval) return; // Rate limit collection

    group('System Resource Monitoring', () => {
      const response = http.get(`${this.monitoringEndpoint}/system`, {
        timeout: '5s',
        tags: { monitoring: 'system_resources' },
      });

      if (
        check(response, { 'System metrics available': r => r.status === 200 })
      ) {
        try {
          const metrics = JSON.parse(response.body);
          this.updateSystemMetrics(metrics);
        } catch (e) {
          console.warn('Failed to parse system metrics:', e.message);
        }
      }
    });

    this.lastCollection = now;
  }

  /**
   * Update system performance metrics
   */
  updateSystemMetrics(metrics) {
    if (metrics.cpu) {
      systemCpuUsage.add(metrics.cpu.usage || 0);
      systemLoadAverage.add(metrics.cpu.loadAverage || 0);
    }

    if (metrics.memory) {
      const memUsagePercent =
        (metrics.memory.used / metrics.memory.total) * 100;
      systemMemoryUsage.add(memUsagePercent);
    }

    if (metrics.disk) {
      const diskUsagePercent = (metrics.disk.used / metrics.disk.total) * 100;
      systemDiskUsage.add(diskUsagePercent);
    }

    if (metrics.network) {
      systemNetworkIn.add(metrics.network.bytesReceived || 0);
      systemNetworkOut.add(metrics.network.bytesSent || 0);
    }

    console.log(
      `📊 System: CPU ${metrics.cpu?.usage?.toFixed(1)}%, Memory ${(((metrics.memory?.used || 0) / (metrics.memory?.total || 1)) * 100).toFixed(1)}%`
    );
  }

  /**
   * Check system health thresholds
   */
  checkSystemHealth(metrics) {
    const alerts = [];

    if (metrics.cpu?.usage > 90) {
      alerts.push({
        type: 'CPU_HIGH',
        value: metrics.cpu.usage,
        threshold: 90,
      });
    }

    if (metrics.memory) {
      const memUsage = (metrics.memory.used / metrics.memory.total) * 100;
      if (memUsage > 85) {
        alerts.push({ type: 'MEMORY_HIGH', value: memUsage, threshold: 85 });
      }
    }

    if (metrics.disk) {
      const diskUsage = (metrics.disk.used / metrics.disk.total) * 100;
      if (diskUsage > 90) {
        alerts.push({ type: 'DISK_HIGH', value: diskUsage, threshold: 90 });
      }
    }

    return alerts;
  }
}

/**
 * Application Performance Monitor
 */
export class ApplicationPerformanceMonitor {
  constructor() {
    this.apmEndpoint = `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/monitoring/apm`;
    this.enabled = __ENV.APM_MONITORING_ENABLED !== 'false';
    this.transactionTraces = [];
  }

  /**
   * Collect APM metrics
   */
  collectAPMMetrics() {
    if (!this.enabled) return;

    group('APM Monitoring', () => {
      const response = http.get(`${this.apmEndpoint}/metrics`, {
        timeout: '5s',
        tags: { monitoring: 'apm' },
      });

      if (check(response, { 'APM metrics available': r => r.status === 200 })) {
        try {
          const metrics = JSON.parse(response.body);
          this.updateAPMMetrics(metrics);
        } catch (e) {
          console.warn('Failed to parse APM metrics:', e.message);
        }
      }
    });
  }

  /**
   * Update application performance metrics
   */
  updateAPMMetrics(metrics) {
    if (metrics.response_time) {
      applicationResponseTime.add(metrics.response_time.avg || 0);
    }

    if (metrics.throughput) {
      applicationThroughput.add(metrics.throughput.requests_per_second || 0);
    }

    if (metrics.error_rate) {
      applicationErrorRate.add(metrics.error_rate.percentage / 100 || 0);
    }

    if (metrics.database) {
      databaseConnectionsActive.add(metrics.database.connections_active || 0);
      databaseConnectionsIdle.add(metrics.database.connections_idle || 0);
    }

    if (metrics.cache) {
      cacheHitRatio.add(metrics.cache.hit_ratio * 100 || 0);
    }
  }

  /**
   * Trace transaction performance
   */
  traceTransaction(transactionId, startTime, endTime, service, operation) {
    const duration = endTime - startTime;
    const trace = {
      id: transactionId,
      service,
      operation,
      duration,
      timestamp: startTime,
      success: duration < 5000, // Consider successful if under 5s
    };

    this.transactionTraces.push(trace);

    // Keep only recent traces (last 1000)
    if (this.transactionTraces.length > 1000) {
      this.transactionTraces = this.transactionTraces.slice(-1000);
    }

    return trace;
  }

  /**
   * Get transaction performance summary
   */
  getTransactionSummary() {
    if (this.transactionTraces.length === 0) return null;

    const durations = this.transactionTraces.map(t => t.duration);
    const successful = this.transactionTraces.filter(t => t.success).length;

    return {
      count: this.transactionTraces.length,
      success_rate: (successful / this.transactionTraces.length) * 100,
      avg_duration: durations.reduce((a, b) => a + b, 0) / durations.length,
      min_duration: Math.min(...durations),
      max_duration: Math.max(...durations),
      p95_duration: this.calculatePercentile(durations, 95),
      p99_duration: this.calculatePercentile(durations, 99),
    };
  }

  calculatePercentile(values, percentile) {
    const sorted = values.slice().sort((a, b) => a - b);
    const index = (percentile / 100) * (sorted.length - 1);

    if (Math.floor(index) === index) {
      return sorted[index];
    }

    const lower = sorted[Math.floor(index)];
    const upper = sorted[Math.ceil(index)];
    return lower + (upper - lower) * (index - Math.floor(index));
  }
}

/**
 * Service-Specific Performance Monitor
 */
export class ServicePerformanceMonitor {
  constructor() {
    this.serviceEndpoints = {
      iot: `${SERVICE_REGISTRY.iot.host}:${SERVICE_REGISTRY.iot.port}/metrics`,
      wallet: `${SERVICE_REGISTRY.wallet.host}:${SERVICE_REGISTRY.wallet.port}/metrics`,
      crossChain: `${SERVICE_REGISTRY.crossChain.host}:${SERVICE_REGISTRY.crossChain.port}/metrics`,
      compute: `${SERVICE_REGISTRY.compute.host}:${SERVICE_REGISTRY.compute.port}/metrics`,
      governance: `${SERVICE_REGISTRY.governance.host}:${SERVICE_REGISTRY.governance.port}/metrics`,
    };
  }

  /**
   * Collect metrics from all services
   */
  collectAllServiceMetrics() {
    group('Service Performance Monitoring', () => {
      Object.entries(this.serviceEndpoints).forEach(
        ([serviceName, endpoint]) => {
          this.collectServiceMetrics(serviceName, endpoint);
        }
      );
    });
  }

  /**
   * Collect metrics from specific service
   */
  collectServiceMetrics(serviceName, endpoint) {
    const response = http.get(endpoint, {
      timeout: '5s',
      tags: { service: serviceName, monitoring: 'service_metrics' },
    });

    if (
      check(response, {
        [`${serviceName} metrics available`]: r => r.status === 200,
      })
    ) {
      try {
        const metrics = JSON.parse(response.body);
        this.updateServiceMetrics(serviceName, metrics);
      } catch (e) {
        console.warn(`Failed to parse ${serviceName} metrics:`, e.message);
      }
    }
  }

  /**
   * Update service-specific metrics
   */
  updateServiceMetrics(serviceName, metrics) {
    switch (serviceName) {
      case 'iot':
        this.updateIOTMetrics(metrics);
        break;
      case 'wallet':
        this.updateWalletMetrics(metrics);
        break;
      case 'crossChain':
        this.updateCrossChainMetrics(metrics);
        break;
    }
  }

  updateIOTMetrics(metrics) {
    if (metrics.sensor_posts_processed) {
      iotServiceMetrics.sensorPostsProcessed.add(
        metrics.sensor_posts_processed
      );
    }
    if (metrics.processing_time_ms) {
      iotServiceMetrics.averageProcessingTime.add(metrics.processing_time_ms);
    }
    if (metrics.queue_depth !== undefined) {
      iotServiceMetrics.queueDepth.add(metrics.queue_depth);
    }
    if (metrics.rejected_posts) {
      iotServiceMetrics.rejectedPosts.add(metrics.rejected_posts);
    }
  }

  updateWalletMetrics(metrics) {
    if (metrics.transactions_processed) {
      walletServiceMetrics.transactionsProcessed.add(
        metrics.transactions_processed
      );
    }
    if (metrics.balance_queries) {
      walletServiceMetrics.balanceQueries.add(metrics.balance_queries);
    }
    if (metrics.transaction_time_ms) {
      walletServiceMetrics.averageTransactionTime.add(
        metrics.transaction_time_ms
      );
    }
  }

  updateCrossChainMetrics(metrics) {
    if (metrics.bridge_operations) {
      crossChainMetrics.bridgeOperations.add(metrics.bridge_operations);
    }
    if (metrics.bridge_latency_ms) {
      crossChainMetrics.bridgeLatency.add(metrics.bridge_latency_ms);
    }
    if (metrics.failed_bridges) {
      crossChainMetrics.failedBridges.add(metrics.failed_bridges);
    }
  }
}

/**
 * Log Aggregation Monitor
 */
export class LogAggregationMonitor {
  constructor() {
    this.logEndpoint = `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/logs`;
    this.errorPatterns = [
      /ERROR/i,
      /FATAL/i,
      /EXCEPTION/i,
      /TIMEOUT/i,
      /CONNECTION.*REFUSED/i,
      /OUT.*OF.*MEMORY/i,
    ];
  }

  /**
   * Collect and analyze recent logs
   */
  collectLogs(timeRange = '5m') {
    group('Log Analysis', () => {
      const response = http.get(
        `${this.logEndpoint}/recent?range=${timeRange}`,
        {
          timeout: '10s',
          tags: { monitoring: 'log_analysis' },
        }
      );

      if (check(response, { 'Logs retrieved': r => r.status === 200 })) {
        try {
          const logs = JSON.parse(response.body);
          this.analyzeLogs(logs);
        } catch (e) {
          console.warn('Failed to parse logs:', e.message);
        }
      }
    });
  }

  /**
   * Analyze logs for errors and patterns
   */
  analyzeLogs(logs) {
    let errorCount = 0;
    let warningCount = 0;
    const errorServices = new Set();

    logs.forEach(logEntry => {
      const message = logEntry.message || '';
      const service = logEntry.service || 'unknown';

      if (this.isErrorLog(message)) {
        errorCount++;
        errorServices.add(service);
      } else if (this.isWarningLog(message)) {
        warningCount++;
      }
    });

    console.log(
      `📋 Log Analysis: ${errorCount} errors, ${warningCount} warnings from ${errorServices.size} services`
    );

    if (errorCount > 10) {
      console.warn(`⚠️ High error rate detected: ${errorCount} errors in logs`);
    }

    if (errorServices.size > 0) {
      console.log(
        `🚨 Services with errors: ${Array.from(errorServices).join(', ')}`
      );
    }
  }

  isErrorLog(message) {
    return this.errorPatterns.some(pattern => pattern.test(message));
  }

  isWarningLog(message) {
    return /WARN/i.test(message);
  }
}

/**
 * Performance Report Generator
 */
export class PerformanceReportGenerator {
  constructor() {
    this.reportData = {
      startTime: Date.now(),
      metrics: {},
      alerts: [],
      summary: {},
    };
  }

  /**
   * Generate comprehensive performance report
   */
  generateReport(systemMonitor, apmMonitor, serviceMonitor) {
    this.reportData.endTime = Date.now();
    this.reportData.duration =
      this.reportData.endTime - this.reportData.startTime;

    // Collect final metrics
    this.reportData.metrics = {
      system: this.getSystemMetricsSummary(),
      application: apmMonitor.getTransactionSummary(),
      services: this.getServiceMetricsSummary(),
    };

    // Generate summary
    this.reportData.summary = this.generateSummary();

    return this.reportData;
  }

  getSystemMetricsSummary() {
    return {
      cpu_usage_max: systemCpuUsage.value || 0,
      memory_usage_max: systemMemoryUsage.value || 0,
      disk_usage_max: systemDiskUsage.value || 0,
      load_average_max: systemLoadAverage.value || 0,
    };
  }

  getServiceMetricsSummary() {
    return {
      iot: {
        posts_processed: iotServiceMetrics.sensorPostsProcessed.count || 0,
        avg_processing_time: iotServiceMetrics.averageProcessingTime.avg || 0,
        rejected_posts: iotServiceMetrics.rejectedPosts.count || 0,
      },
      wallet: {
        transactions_processed:
          walletServiceMetrics.transactionsProcessed.count || 0,
        balance_queries: walletServiceMetrics.balanceQueries.count || 0,
        avg_transaction_time:
          walletServiceMetrics.averageTransactionTime.avg || 0,
      },
      cross_chain: {
        bridge_operations: crossChainMetrics.bridgeOperations.count || 0,
        avg_bridge_latency: crossChainMetrics.bridgeLatency.avg || 0,
        failed_bridges: crossChainMetrics.failedBridges.count || 0,
      },
    };
  }

  generateSummary() {
    return {
      test_duration_minutes: (this.reportData.duration / 60000).toFixed(2),
      overall_health: this.calculateOverallHealth(),
      recommendations: this.generateRecommendations(),
    };
  }

  calculateOverallHealth() {
    const cpuHealth = systemCpuUsage.value < 80 ? 'good' : 'poor';
    const memoryHealth = systemMemoryUsage.value < 85 ? 'good' : 'poor';
    const appHealth = applicationErrorRate.rate < 0.05 ? 'good' : 'poor';

    const healthScores = [cpuHealth, memoryHealth, appHealth];
    const goodCount = healthScores.filter(h => h === 'good').length;

    if (goodCount === 3) return 'excellent';
    if (goodCount === 2) return 'good';
    if (goodCount === 1) return 'fair';
    return 'poor';
  }

  generateRecommendations() {
    const recommendations = [];

    if (systemCpuUsage.value > 80) {
      recommendations.push(
        'Consider scaling up CPU resources or optimizing application performance'
      );
    }

    if (systemMemoryUsage.value > 85) {
      recommendations.push(
        'Memory usage is high - investigate memory leaks or increase available memory'
      );
    }

    if (applicationErrorRate.rate > 0.05) {
      recommendations.push(
        'Error rate is elevated - review application logs and fix underlying issues'
      );
    }

    if (databaseConnectionsActive.value > 80) {
      recommendations.push('Database connection pool may need tuning');
    }

    if (cacheHitRatio.value < 70) {
      recommendations.push('Cache hit ratio is low - review caching strategy');
    }

    return recommendations;
  }
}

/**
 * Main Performance Monitoring Manager
 */
export class PerformanceMonitoringManager {
  constructor() {
    this.systemMonitor = new SystemResourceMonitor();
    this.apmMonitor = new ApplicationPerformanceMonitor();
    this.serviceMonitor = new ServicePerformanceMonitor();
    this.logMonitor = new LogAggregationMonitor();
    this.reportGenerator = new PerformanceReportGenerator();
  }

  /**
   * Start monitoring during load test
   */
  startMonitoring() {
    console.log('🔍 Starting performance monitoring...');

    // Collect metrics periodically
    this.monitoringInterval = setInterval(() => {
      this.collectAllMetrics();
    }, 10000); // Every 10 seconds
  }

  /**
   * Stop monitoring and generate report
   */
  stopMonitoring() {
    if (this.monitoringInterval) {
      clearInterval(this.monitoringInterval);
    }

    console.log('📊 Generating performance report...');
    return this.reportGenerator.generateReport(
      this.systemMonitor,
      this.apmMonitor,
      this.serviceMonitor
    );
  }

  /**
   * Collect all performance metrics
   */
  collectAllMetrics() {
    this.systemMonitor.collectSystemMetrics();
    this.apmMonitor.collectAPMMetrics();
    this.serviceMonitor.collectAllServiceMetrics();
    this.logMonitor.collectLogs();
  }
}

export {
  systemCpuUsage,
  systemMemoryUsage,
  applicationResponseTime,
  iotServiceMetrics,
  walletServiceMetrics,
  crossChainMetrics,
};
