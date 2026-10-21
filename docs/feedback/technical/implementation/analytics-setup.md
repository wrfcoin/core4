# Documentation Analytics and Monitoring Setup

This guide provides comprehensive instructions for setting up analytics, monitoring, and reporting
systems for the documentation feedback infrastructure.

## Overview

The analytics system provides:

- **Real-time Monitoring** of documentation usage and feedback
- **Performance Metrics** for content effectiveness
- **User Behavior Analysis** to identify improvement opportunities
- **Automated Alerting** for critical issues
- **Executive Reporting** with actionable insights

## 1. Analytics Dashboard Setup

### React Admin Dashboard

```typescript
// src/admin/DocumentationDashboard.tsx
import React, { useState, useEffect } from 'react';
import {
  LineChart, Line, BarChart, Bar, PieChart, Pie, Cell,
  XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer
} from 'recharts';
import { getAnalytics, getFeedbackSummary, getTopPages } from '../services/analytics';

interface DashboardMetrics {
  totalViews: number;
  uniqueVisitors: number;
  averageSessionDuration: number;
  feedbackSatisfaction: number;
  topPages: Array<{
    pageId: string;
    title: string;
    views: number;
    satisfaction: number;
  }>;
  feedbackTrends: Array<{
    date: string;
    positive: number;
    negative: number;
    suggestions: number;
    bugs: number;
  }>;
  usagePatterns: Array<{
    hour: number;
    views: number;
    feedback: number;
  }>;
}

export const DocumentationDashboard: React.FC = () => {
  const [metrics, setMetrics] = useState<DashboardMetrics | null>(null);
  const [timeframe, setTimeframe] = useState<'day' | 'week' | 'month'>('week');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    loadDashboardData();
  }, [timeframe]);

  const loadDashboardData = async () => {
    setLoading(true);
    try {
      const [analytics, feedback, topPages] = await Promise.all([
        getAnalytics(timeframe),
        getFeedbackSummary(timeframe),
        getTopPages(timeframe)
      ]);

      setMetrics({
        totalViews: analytics.totalViews,
        uniqueVisitors: analytics.uniqueVisitors,
        averageSessionDuration: analytics.averageSessionDuration,
        feedbackSatisfaction: feedback.satisfactionScore,
        topPages,
        feedbackTrends: feedback.trends,
        usagePatterns: analytics.hourlyPatterns
      });
    } catch (error) {
      console.error('Failed to load dashboard data:', error);
    } finally {
      setLoading(false);
    }
  };

  if (loading) {
    return <div className="dashboard-loading">Loading analytics...</div>;
  }

  return (
    <div className="documentation-dashboard">
      <header className="dashboard-header">
        <h1>Documentation Analytics Dashboard</h1>
        <div className="timeframe-selector">
          <select
            value={timeframe}
            onChange={(e) => setTimeframe(e.target.value as any)}
          >
            <option value="day">Last 24 Hours</option>
            <option value="week">Last 7 Days</option>
            <option value="month">Last 30 Days</option>
          </select>
        </div>
      </header>

      <div className="metrics-overview">
        <div className="metric-card">
          <h3>Total Page Views</h3>
          <div className="metric-value">{metrics?.totalViews.toLocaleString()}</div>
        </div>
        <div className="metric-card">
          <h3>Unique Visitors</h3>
          <div className="metric-value">{metrics?.uniqueVisitors.toLocaleString()}</div>
        </div>
        <div className="metric-card">
          <h3>Avg. Session Duration</h3>
          <div className="metric-value">
            {Math.round((metrics?.averageSessionDuration || 0) / 60)}m
          </div>
        </div>
        <div className="metric-card">
          <h3>Satisfaction Score</h3>
          <div className="metric-value">
            {(metrics?.feedbackSatisfaction || 0).toFixed(1)}/5.0
          </div>
        </div>
      </div>

      <div className="charts-grid">
        <div className="chart-container">
          <h3>Feedback Trends</h3>
          <ResponsiveContainer width="100%" height={300}>
            <LineChart data={metrics?.feedbackTrends}>
              <CartesianGrid strokeDasharray="3 3" />
              <XAxis dataKey="date" />
              <YAxis />
              <Tooltip />
              <Legend />
              <Line type="monotone" dataKey="positive" stroke="#28a745" />
              <Line type="monotone" dataKey="negative" stroke="#dc3545" />
              <Line type="monotone" dataKey="suggestions" stroke="#007bff" />
              <Line type="monotone" dataKey="bugs" stroke="#ffc107" />
            </LineChart>
          </ResponsiveContainer>
        </div>

        <div className="chart-container">
          <h3>Usage Patterns (24 Hour)</h3>
          <ResponsiveContainer width="100%" height={300}>
            <BarChart data={metrics?.usagePatterns}>
              <CartesianGrid strokeDasharray="3 3" />
              <XAxis dataKey="hour" />
              <YAxis />
              <Tooltip />
              <Bar dataKey="views" fill="#007bff" />
            </BarChart>
          </ResponsiveContainer>
        </div>

        <div className="chart-container">
          <h3>Top Performing Pages</h3>
          <div className="top-pages-list">
            {metrics?.topPages.map((page, index) => (
              <div key={page.pageId} className="page-item">
                <div className="page-rank">#{index + 1}</div>
                <div className="page-info">
                  <div className="page-title">{page.title}</div>
                  <div className="page-stats">
                    {page.views} views • {page.satisfaction.toFixed(1)}/5.0 satisfaction
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};
```

### Analytics Service

```typescript
// src/services/analytics.ts
import { API_BASE_URL } from '../config';

export interface AnalyticsData {
  totalViews: number;
  uniqueVisitors: number;
  averageSessionDuration: number;
  bounceRate: number;
  searchSuccessRate: number;
  hourlyPatterns: Array<{
    hour: number;
    views: number;
    feedback: number;
  }>;
}

export interface FeedbackSummary {
  satisfactionScore: number;
  totalFeedback: number;
  positivePercentage: number;
  negativePercentage: number;
  trends: Array<{
    date: string;
    positive: number;
    negative: number;
    suggestions: number;
    bugs: number;
  }>;
}

export async function getAnalytics(timeframe: string): Promise<AnalyticsData> {
  const response = await fetch(`${API_BASE_URL}/analytics/summary?timeframe=${timeframe}`);
  if (!response.ok) {
    throw new Error('Failed to fetch analytics data');
  }
  return response.json();
}

export async function getFeedbackSummary(timeframe: string): Promise<FeedbackSummary> {
  const response = await fetch(`${API_BASE_URL}/feedback/summary?timeframe=${timeframe}`);
  if (!response.ok) {
    throw new Error('Failed to fetch feedback summary');
  }
  return response.json();
}

export async function getTopPages(timeframe: string) {
  const response = await fetch(`${API_BASE_URL}/analytics/top-pages?timeframe=${timeframe}`);
  if (!response.ok) {
    throw new Error('Failed to fetch top pages');
  }
  return response.json();
}

export async function getPageAnalytics(pageId: string, timeframe: string) {
  const response = await fetch(`${API_BASE_URL}/analytics/page/${pageId}?timeframe=${timeframe}`);
  if (!response.ok) {
    throw new Error('Failed to fetch page analytics');
  }
  return response.json();
}

// Real-time analytics with WebSocket
export class RealTimeAnalytics {
  private ws: WebSocket | null = null;
  private listeners: Map<string, Function[]> = new Map();

  connect() {
    const wsUrl = `${API_BASE_URL.replace('http', 'ws')}/analytics/realtime`;
    this.ws = new WebSocket(wsUrl);

    this.ws.onmessage = event => {
      const data = JSON.parse(event.data);
      this.emit(data.type, data.payload);
    };

    this.ws.onclose = () => {
      // Reconnect after 5 seconds
      setTimeout(() => this.connect(), 5000);
    };
  }

  on(event: string, callback: Function) {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, []);
    }
    this.listeners.get(event)!.push(callback);
  }

  private emit(event: string, data: any) {
    const callbacks = this.listeners.get(event) || [];
    callbacks.forEach(callback => callback(data));
  }

  disconnect() {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }
}
```

## 2. Backend Analytics Endpoints

```typescript
// src/api/analytics.ts
import express from 'express';
import { AnalyticsService } from '../services/AnalyticsService';
import { FeedbackService } from '../services/FeedbackService';

const router = express.Router();
const analyticsService = new AnalyticsService();
const feedbackService = new FeedbackService();

// Analytics summary endpoint
router.get('/analytics/summary', async (req, res) => {
  try {
    const { timeframe = 'week' } = req.query;

    const [usage, performance, patterns] = await Promise.all([
      analyticsService.getUsageStatistics(timeframe as string),
      analyticsService.getPerformanceMetrics(timeframe as string),
      analyticsService.getUsagePatterns(timeframe as string),
    ]);

    res.json({
      totalViews: usage.page_views,
      uniqueVisitors: usage.unique_visitors,
      averageSessionDuration: usage.avg_session_duration,
      bounceRate: usage.bounce_rate,
      searchSuccessRate: performance.search_success_rate,
      hourlyPatterns: patterns,
    });
  } catch (error) {
    console.error('Error fetching analytics summary:', error);
    res.status(500).json({ error: 'Failed to fetch analytics' });
  }
});

// Top performing pages
router.get('/analytics/top-pages', async (req, res) => {
  try {
    const { timeframe = 'week', limit = 10 } = req.query;
    const topPages = await analyticsService.getTopPages(
      timeframe as string,
      parseInt(limit as string)
    );
    res.json(topPages);
  } catch (error) {
    console.error('Error fetching top pages:', error);
    res.status(500).json({ error: 'Failed to fetch top pages' });
  }
});

// Page-specific analytics
router.get('/analytics/page/:pageId', async (req, res) => {
  try {
    const { pageId } = req.params;
    const { timeframe = 'week' } = req.query;

    const analytics = await analyticsService.getPageAnalytics(pageId, timeframe as string);
    res.json(analytics);
  } catch (error) {
    console.error('Error fetching page analytics:', error);
    res.status(500).json({ error: 'Failed to fetch page analytics' });
  }
});

// Feedback summary
router.get('/feedback/summary', async (req, res) => {
  try {
    const { timeframe = 'week' } = req.query;
    const summary = await feedbackService.getFeedbackSummary(timeframe as string);
    res.json(summary);
  } catch (error) {
    console.error('Error fetching feedback summary:', error);
    res.status(500).json({ error: 'Failed to fetch feedback summary' });
  }
});

// Search analytics
router.get('/analytics/search', async (req, res) => {
  try {
    const { timeframe = 'week' } = req.query;
    const searchAnalytics = await analyticsService.getSearchAnalytics(timeframe as string);
    res.json(searchAnalytics);
  } catch (error) {
    console.error('Error fetching search analytics:', error);
    res.status(500).json({ error: 'Failed to fetch search analytics' });
  }
});

export default router;
```

## 3. Enhanced Analytics Service

```typescript
// src/services/AnalyticsService.ts (Enhanced)
import { dbPool } from '../database/connection';

export class AnalyticsService {
  async getUsageStatistics(timeframe: string) {
    const timeCondition = this.getTimeCondition(timeframe);

    const query = `
      WITH session_stats AS (
        SELECT 
          session_id,
          MIN(created_at) as session_start,
          MAX(created_at) as session_end,
          COUNT(DISTINCT page_id) as pages_viewed
        FROM analytics_events 
        WHERE event_type = 'page_view' AND ${timeCondition}
        GROUP BY session_id
      ),
      session_durations AS (
        SELECT 
          session_id,
          pages_viewed,
          EXTRACT(EPOCH FROM (session_end - session_start)) as duration_seconds
        FROM session_stats
      )
      SELECT 
        COUNT(DISTINCT e.session_id) as unique_visitors,
        COUNT(*) as page_views,
        AVG(sd.duration_seconds) as avg_session_duration,
        AVG(CASE WHEN sd.pages_viewed = 1 THEN 1.0 ELSE 0.0 END) as bounce_rate
      FROM analytics_events e
      LEFT JOIN session_durations sd ON e.session_id = sd.session_id
      WHERE e.event_type = 'page_view' AND ${timeCondition}
    `;

    const result = await dbPool.query(query);
    return result.rows[0];
  }

  async getPerformanceMetrics(timeframe: string) {
    const timeCondition = this.getTimeCondition(timeframe);

    const query = `
      SELECT 
        AVG(CASE WHEN event_data->>'page_load_time' IS NOT NULL 
            THEN (event_data->>'page_load_time')::float END) as avg_page_load_time,
        AVG(CASE WHEN event_data->>'search_results_count' IS NOT NULL 
            THEN CASE WHEN (event_data->>'search_results_count')::int > 0 
                 THEN 1.0 ELSE 0.0 END END) as search_success_rate,
        COUNT(CASE WHEN event_type = 'error' THEN 1 END) as error_count,
        COUNT(*) as total_events
      FROM analytics_events 
      WHERE ${timeCondition}
    `;

    const result = await dbPool.query(query);
    return result.rows[0];
  }

  async getUsagePatterns(timeframe: string) {
    const timeCondition = this.getTimeCondition(timeframe);

    const query = `
      SELECT 
        EXTRACT(HOUR FROM created_at) as hour,
        COUNT(CASE WHEN event_type = 'page_view' THEN 1 END) as views,
        COUNT(CASE WHEN event_type = 'feedback_submitted' THEN 1 END) as feedback
      FROM analytics_events 
      WHERE ${timeCondition}
      GROUP BY EXTRACT(HOUR FROM created_at)
      ORDER BY hour
    `;

    const result = await dbPool.query(query);
    return result.rows;
  }

  async getTopPages(timeframe: string, limit: number) {
    const timeCondition = this.getTimeCondition(timeframe);

    const query = `
      WITH page_stats AS (
        SELECT 
          e.page_id,
          p.title,
          COUNT(CASE WHEN e.event_type = 'page_view' THEN 1 END) as views,
          AVG(CASE WHEN f.feedback_type = 'positive' THEN 5.0
                   WHEN f.feedback_type = 'negative' THEN 1.0
                   ELSE 3.0 END) as satisfaction
        FROM analytics_events e
        LEFT JOIN documentation_pages p ON e.page_id = p.id
        LEFT JOIN feedback_entries f ON e.page_id = f.page_id 
          AND f.created_at >= NOW() - INTERVAL '${timeframe}'
        WHERE e.${timeCondition}
        GROUP BY e.page_id, p.title
      )
      SELECT * FROM page_stats
      ORDER BY views DESC
      LIMIT $1
    `;

    const result = await dbPool.query(query, [limit]);
    return result.rows;
  }

  async getSearchAnalytics(timeframe: string) {
    const timeCondition = this.getTimeCondition(timeframe);

    const query = `
      SELECT 
        event_data->>'search_query' as query,
        COUNT(*) as search_count,
        AVG(CASE WHEN (event_data->>'results_count')::int > 0 
            THEN 1.0 ELSE 0.0 END) as success_rate
      FROM analytics_events 
      WHERE event_type = 'search_performed' 
        AND event_data->>'search_query' IS NOT NULL
        AND ${timeCondition}
      GROUP BY event_data->>'search_query'
      ORDER BY search_count DESC
      LIMIT 20
    `;

    const result = await dbPool.query(query);
    return result.rows;
  }

  async generateHealthScore(): Promise<number> {
    // Calculate overall documentation health score
    const [usage, feedback, performance] = await Promise.all([
      this.getUsageStatistics('week'),
      this.getRecentFeedback(),
      this.getPerformanceMetrics('week'),
    ]);

    // Health score calculation (0-100)
    let score = 100;

    // Deduct for high bounce rate
    if (usage.bounce_rate > 0.7) score -= 20;
    else if (usage.bounce_rate > 0.5) score -= 10;

    // Deduct for low satisfaction
    if (feedback.satisfaction_score < 3.0) score -= 30;
    else if (feedback.satisfaction_score < 4.0) score -= 15;

    // Deduct for performance issues
    if (performance.avg_page_load_time > 3000) score -= 15;
    else if (performance.avg_page_load_time > 2000) score -= 10;

    // Deduct for search issues
    if (performance.search_success_rate < 0.8) score -= 10;

    return Math.max(0, score);
  }

  private async getRecentFeedback() {
    const query = `
      SELECT 
        AVG(CASE WHEN feedback_type = 'positive' THEN 5.0
                 WHEN feedback_type = 'negative' THEN 1.0
                 ELSE 3.0 END) as satisfaction_score,
        COUNT(*) as total_feedback
      FROM feedback_entries 
      WHERE created_at >= NOW() - INTERVAL '1 week'
    `;

    const result = await dbPool.query(query);
    return result.rows[0];
  }

  private getTimeCondition(timeframe: string): string {
    switch (timeframe) {
      case 'hour':
        return "created_at >= NOW() - INTERVAL '1 hour'";
      case 'day':
        return "created_at >= NOW() - INTERVAL '1 day'";
      case 'week':
        return "created_at >= NOW() - INTERVAL '1 week'";
      case 'month':
        return "created_at >= NOW() - INTERVAL '1 month'";
      default:
        return "created_at >= NOW() - INTERVAL '1 week'";
    }
  }
}
```

## 4. Monitoring and Alerting

### Prometheus Metrics Configuration

```typescript
// src/monitoring/metrics.ts
import prometheus from 'prom-client';

// Create a Registry which registers the metrics
const register = new prometheus.Registry();

// Add a default label which is added to all metrics
register.setDefaultLabels({
  app: 'wrfcoin-docs-feedback',
});

// Enable the collection of default metrics
prometheus.collectDefaultMetrics({ register });

// Custom metrics
export const feedbackCounter = new prometheus.Counter({
  name: 'documentation_feedback_total',
  help: 'Total number of feedback submissions',
  labelNames: ['type', 'page_section', 'user_role'],
  registers: [register],
});

export const pageViewCounter = new prometheus.Counter({
  name: 'documentation_page_views_total',
  help: 'Total number of page views',
  labelNames: ['page_id', 'user_agent_type'],
  registers: [register],
});

export const feedbackResponseTime = new prometheus.Histogram({
  name: 'feedback_response_time_seconds',
  help: 'Time taken to respond to feedback',
  labelNames: ['priority', 'category'],
  buckets: [0.1, 0.5, 1, 2, 5, 10, 30, 60, 300, 600],
  registers: [register],
});

export const documentationHealthGauge = new prometheus.Gauge({
  name: 'documentation_health_score',
  help: 'Overall documentation health score (0-100)',
  registers: [register],
});

export const activeUsersGauge = new prometheus.Gauge({
  name: 'documentation_active_users',
  help: 'Number of active users in the last 24 hours',
  registers: [register],
});

// Metrics endpoint
export function getMetrics() {
  return register.metrics();
}

// Update metrics periodically
export async function updateHealthMetrics() {
  try {
    const analyticsService = new AnalyticsService();
    const healthScore = await analyticsService.generateHealthScore();
    documentationHealthGauge.set(healthScore);

    const usage = await analyticsService.getUsageStatistics('day');
    activeUsersGauge.set(usage.unique_visitors || 0);
  } catch (error) {
    console.error('Failed to update health metrics:', error);
  }
}

// Update health metrics every 5 minutes
setInterval(updateHealthMetrics, 5 * 60 * 1000);
```

### Alert Configuration

```yaml
# alerts/documentation-alerts.yml
groups:
  - name: documentation.rules
    interval: 30s
    rules:
      - alert: DocumentationHealthLow
        expr: documentation_health_score < 60
        for: 5m
        labels:
          severity: warning
          team: documentation
        annotations:
          summary: 'Documentation health score is low'
          description: 'Health score has been below 60 for more than 5 minutes'

      - alert: HighNegativeFeedback
        expr:
          ( rate(documentation_feedback_total{type="negative"}[5m]) /
          rate(documentation_feedback_total[5m]) ) > 0.3
        for: 2m
        labels:
          severity: warning
          team: documentation
        annotations:
          summary: 'High rate of negative feedback'
          description: 'More than 30% of feedback is negative in the last 5 minutes'

      - alert: FeedbackResponseSlow
        expr: histogram_quantile(0.95, rate(feedback_response_time_seconds_bucket[5m])) > 86400
        for: 10m
        labels:
          severity: critical
          team: documentation
        annotations:
          summary: 'Slow feedback response time'
          description: '95th percentile feedback response time is over 24 hours'

      - alert: DocumentationDown
        expr: up{job="documentation-feedback"} == 0
        for: 1m
        labels:
          severity: critical
          team: documentation
        annotations:
          summary: 'Documentation feedback service is down'
          description: 'The documentation feedback service has been down for more than 1 minute'
```

### Grafana Dashboard Configuration

```json
{
  "dashboard": {
    "id": null,
    "title": "WRFCoin Documentation Analytics",
    "tags": ["documentation", "feedback"],
    "timezone": "browser",
    "refresh": "30s",
    "panels": [
      {
        "id": 1,
        "title": "Documentation Health Score",
        "type": "stat",
        "targets": [
          {
            "expr": "documentation_health_score",
            "legendFormat": "Health Score"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "min": 0,
            "max": 100,
            "unit": "percent",
            "thresholds": {
              "steps": [
                { "color": "red", "value": 0 },
                { "color": "yellow", "value": 60 },
                { "color": "green", "value": 80 }
              ]
            }
          }
        }
      },
      {
        "id": 2,
        "title": "Page Views",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(documentation_page_views_total[5m])",
            "legendFormat": "Page Views/sec"
          }
        ]
      },
      {
        "id": 3,
        "title": "Feedback Distribution",
        "type": "piechart",
        "targets": [
          {
            "expr": "sum by (type) (documentation_feedback_total)",
            "legendFormat": "{{type}}"
          }
        ]
      },
      {
        "id": 4,
        "title": "Feedback Response Time",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.50, rate(feedback_response_time_seconds_bucket[5m]))",
            "legendFormat": "50th percentile"
          },
          {
            "expr": "histogram_quantile(0.95, rate(feedback_response_time_seconds_bucket[5m]))",
            "legendFormat": "95th percentile"
          }
        ]
      }
    ]
  }
}
```

## 5. Automated Reporting

### Weekly Report Generator

```typescript
// src/reporting/WeeklyReportGenerator.ts
import { AnalyticsService } from '../services/AnalyticsService';
import { FeedbackService } from '../services/FeedbackService';
import { EmailService } from '../services/EmailService';

export class WeeklyReportGenerator {
  private analyticsService = new AnalyticsService();
  private feedbackService = new FeedbackService();
  private emailService = new EmailService();

  async generateAndSendReport() {
    try {
      const report = await this.generateReport();
      await this.sendReport(report);
    } catch (error) {
      console.error('Failed to generate weekly report:', error);
    }
  }

  private async generateReport() {
    const [analytics, feedback, topPages, healthScore] = await Promise.all([
      this.analyticsService.getUsageStatistics('week'),
      this.feedbackService.getFeedbackSummary('week'),
      this.analyticsService.getTopPages('week', 10),
      this.analyticsService.generateHealthScore(),
    ]);

    return {
      period: {
        start: new Date(Date.now() - 7 * 24 * 60 * 60 * 1000),
        end: new Date(),
      },
      overview: {
        healthScore,
        totalViews: analytics.page_views,
        uniqueVisitors: analytics.unique_visitors,
        averageSessionDuration: analytics.avg_session_duration,
        bounceRate: analytics.bounce_rate,
      },
      feedback: {
        total: feedback.total_feedback,
        positive: feedback.positive,
        negative: feedback.negative,
        suggestions: feedback.suggestions,
        bugs: feedback.bugs,
        satisfactionScore: feedback.satisfaction_score,
      },
      topPages,
      insights: await this.generateInsights(analytics, feedback),
    };
  }

  private async generateInsights(analytics: any, feedback: any) {
    const insights = [];

    // Bounce rate insights
    if (analytics.bounce_rate > 0.7) {
      insights.push({
        type: 'warning',
        title: 'High Bounce Rate',
        description: `Bounce rate is ${(analytics.bounce_rate * 100).toFixed(1)}%, indicating users may not be finding what they need quickly.`,
        recommendation:
          'Consider improving page navigation and adding better search functionality.',
      });
    }

    // Feedback insights
    const negativePercentage = (feedback.negative / feedback.total_feedback) * 100;
    if (negativePercentage > 20) {
      insights.push({
        type: 'warning',
        title: 'High Negative Feedback',
        description: `${negativePercentage.toFixed(1)}% of feedback is negative.`,
        recommendation:
          'Review recent negative feedback to identify common issues and prioritize fixes.',
      });
    }

    // Positive trends
    if (feedback.satisfaction_score > 4.0) {
      insights.push({
        type: 'success',
        title: 'High Satisfaction',
        description: `Documentation satisfaction score is ${feedback.satisfaction_score.toFixed(1)}/5.0.`,
        recommendation: 'Continue current documentation quality practices.',
      });
    }

    return insights;
  }

  private async sendReport(report: any) {
    const htmlContent = this.generateEmailHTML(report);

    await this.emailService.send({
      to: ['documentation-team@wrfcoin.org', 'management@wrfcoin.org'],
      subject: `Weekly Documentation Report - ${report.period.start.toDateString()}`,
      html: htmlContent,
    });
  }

  private generateEmailHTML(report: any): string {
    return `
      <!DOCTYPE html>
      <html>
      <head>
        <style>
          body { font-family: Arial, sans-serif; margin: 20px; }
          .header { background: #007bff; color: white; padding: 20px; border-radius: 5px; }
          .metric { display: inline-block; margin: 10px; padding: 15px; background: #f8f9fa; border-radius: 5px; text-align: center; }
          .metric-value { font-size: 24px; font-weight: bold; color: #007bff; }
          .insight { margin: 10px 0; padding: 15px; border-left: 4px solid; }
          .insight.warning { border-color: #ffc107; background: #fff3cd; }
          .insight.success { border-color: #28a745; background: #d4edda; }
          .top-pages { margin-top: 20px; }
          .page-item { padding: 10px; border-bottom: 1px solid #eee; }
        </style>
      </head>
      <body>
        <div class="header">
          <h1>📊 Weekly Documentation Report</h1>
          <p>Period: ${report.period.start.toDateString()} - ${report.period.end.toDateString()}</p>
        </div>

        <h2>📈 Key Metrics</h2>
        <div class="metrics">
          <div class="metric">
            <div class="metric-value">${report.overview.healthScore}</div>
            <div>Health Score</div>
          </div>
          <div class="metric">
            <div class="metric-value">${report.overview.totalViews.toLocaleString()}</div>
            <div>Page Views</div>
          </div>
          <div class="metric">
            <div class="metric-value">${report.overview.uniqueVisitors.toLocaleString()}</div>
            <div>Unique Visitors</div>
          </div>
          <div class="metric">
            <div class="metric-value">${Math.round(report.overview.averageSessionDuration / 60)}m</div>
            <div>Avg. Session</div>
          </div>
        </div>

        <h2>💬 Feedback Summary</h2>
        <p>Total Feedback: <strong>${report.feedback.total}</strong></p>
        <ul>
          <li>👍 Positive: ${report.feedback.positive}</li>
          <li>👎 Negative: ${report.feedback.negative}</li>
          <li>💡 Suggestions: ${report.feedback.suggestions}</li>
          <li>🐛 Bugs: ${report.feedback.bugs}</li>
        </ul>
        <p>Satisfaction Score: <strong>${report.feedback.satisfactionScore.toFixed(1)}/5.0</strong></p>

        <h2>🔍 Insights & Recommendations</h2>
        ${report.insights
          .map(
            insight => `
          <div class="insight ${insight.type}">
            <h4>${insight.title}</h4>
            <p><strong>Observation:</strong> ${insight.description}</p>
            <p><strong>Recommendation:</strong> ${insight.recommendation}</p>
          </div>
        `
          )
          .join('')}

        <h2>🏆 Top Performing Pages</h2>
        <div class="top-pages">
          ${report.topPages
            .slice(0, 5)
            .map(
              (page, index) => `
            <div class="page-item">
              <strong>#${index + 1}</strong> ${page.title}
              <br>
              <small>${page.views} views • ${page.satisfaction.toFixed(1)}/5.0 satisfaction</small>
            </div>
          `
            )
            .join('')}
        </div>

        <p><em>Generated automatically by WRFCoin Documentation Analytics System</em></p>
      </body>
      </html>
    `;
  }
}

// Schedule weekly reports
export function scheduleWeeklyReports() {
  const generator = new WeeklyReportGenerator();

  // Run every Monday at 9 AM
  const cron = require('node-cron');
  cron.schedule('0 9 * * 1', async () => {
    console.log('Generating weekly documentation report...');
    await generator.generateAndSendReport();
  });
}
```

---

**Next Steps:**

1. [Set up GitHub workflow integrations](./github-integration.md)
2. [Configure team training materials](./team-training.md)
3. [Launch community contribution program](./community-program.md)

**Last Updated**: January 2025  
**Version**: 1.0.0
