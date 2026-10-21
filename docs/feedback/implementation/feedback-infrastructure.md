# Documentation Feedback Infrastructure Setup

This guide provides step-by-step instructions for implementing the documentation feedback collection
and processing infrastructure.

## Prerequisites

- Node.js 18+ with npm or yarn
- PostgreSQL database for feedback storage
- Redis for caching and session management
- GitHub repository with Actions enabled
- Slack workspace for team notifications

## 1. Database Schema Setup

### PostgreSQL Tables

```sql
-- Feedback entries table
CREATE TABLE feedback_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id VARCHAR(255) NOT NULL,
    feedback_type VARCHAR(50) NOT NULL CHECK (feedback_type IN ('positive', 'negative', 'suggestion', 'bug')),
    content TEXT,
    priority VARCHAR(20) DEFAULT 'medium' CHECK (priority IN ('low', 'medium', 'high', 'urgent')),
    category VARCHAR(100),
    status VARCHAR(20) DEFAULT 'open' CHECK (status IN ('open', 'in_progress', 'resolved', 'closed')),

    -- User information
    user_id VARCHAR(255),
    user_role VARCHAR(50),
    user_experience VARCHAR(20),

    -- Metadata
    user_agent TEXT,
    page_load_time INTEGER,
    previous_pages TEXT[],
    session_id VARCHAR(255),
    ip_address INET,

    -- Timestamps
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    resolved_at TIMESTAMP WITH TIME ZONE,

    -- Indexing
    CONSTRAINT fk_feedback_page FOREIGN KEY (page_id) REFERENCES documentation_pages(id)
);

-- Documentation pages table
CREATE TABLE documentation_pages (
    id VARCHAR(255) PRIMARY KEY,
    title VARCHAR(500) NOT NULL,
    url TEXT NOT NULL,
    section VARCHAR(100),
    subsection VARCHAR(100),
    content_hash VARCHAR(64),
    last_modified TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    view_count INTEGER DEFAULT 0,
    avg_time_on_page INTEGER DEFAULT 0,
    bounce_rate DECIMAL(5,2) DEFAULT 0,

    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Analytics events table
CREATE TABLE analytics_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type VARCHAR(100) NOT NULL,
    page_id VARCHAR(255),
    user_id VARCHAR(255),
    session_id VARCHAR(255),

    -- Event data
    event_data JSONB,

    -- Context
    user_agent TEXT,
    ip_address INET,
    referrer TEXT,

    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),

    CONSTRAINT fk_analytics_page FOREIGN KEY (page_id) REFERENCES documentation_pages(id)
);

-- Feedback processing assignments
CREATE TABLE feedback_assignments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    feedback_id UUID NOT NULL,
    assigned_to VARCHAR(255) NOT NULL,
    assigned_by VARCHAR(255) NOT NULL,
    notes TEXT,

    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),

    CONSTRAINT fk_assignment_feedback FOREIGN KEY (feedback_id) REFERENCES feedback_entries(id)
);

-- Create indexes for performance
CREATE INDEX idx_feedback_page_id ON feedback_entries(page_id);
CREATE INDEX idx_feedback_type ON feedback_entries(feedback_type);
CREATE INDEX idx_feedback_priority ON feedback_entries(priority);
CREATE INDEX idx_feedback_status ON feedback_entries(status);
CREATE INDEX idx_feedback_created_at ON feedback_entries(created_at);

CREATE INDEX idx_analytics_event_type ON analytics_events(event_type);
CREATE INDEX idx_analytics_page_id ON analytics_events(page_id);
CREATE INDEX idx_analytics_created_at ON analytics_events(created_at);

CREATE INDEX idx_pages_section ON documentation_pages(section);
CREATE INDEX idx_pages_last_modified ON documentation_pages(last_modified);
```

### Database Connection Configuration

```typescript
// src/database/connection.ts
import { Pool } from 'pg';
import { createClient } from 'redis';

export const dbPool = new Pool({
  host: process.env.DB_HOST || 'localhost',
  port: parseInt(process.env.DB_PORT || '5432'),
  database: process.env.DB_NAME || 'wrfcoin_docs',
  user: process.env.DB_USER || 'postgres',
  password: process.env.DB_PASSWORD,
  max: 20,
  idleTimeoutMillis: 30000,
  connectionTimeoutMillis: 2000,
});

export const redisClient = createClient({
  url: process.env.REDIS_URL || 'redis://localhost:6379',
});

// Initialize connections
export async function initializeDatabase() {
  try {
    await dbPool.connect();
    await redisClient.connect();
    console.log('Database connections established');
  } catch (error) {
    console.error('Failed to connect to database:', error);
    process.exit(1);
  }
}
```

## 2. Backend API Service

### Express.js Feedback API

```typescript
// src/api/feedback.ts
import express from 'express';
import { body, validationResult } from 'express-validator';
import { FeedbackService } from '../services/FeedbackService';
import { AnalyticsService } from '../services/AnalyticsService';
import { rateLimiter } from '../middleware/rateLimiter';

const router = express.Router();
const feedbackService = new FeedbackService();
const analyticsService = new AnalyticsService();

// Submit feedback
router.post(
  '/feedback',
  rateLimiter({
    windowMs: 15 * 60 * 1000, // 15 minutes
    max: 10, // Limit each IP to 10 requests per windowMs
    message: 'Too many feedback submissions, please try again later',
  }),
  [
    body('pageId').notEmpty().isLength({ max: 255 }),
    body('type').isIn(['positive', 'negative', 'suggestion', 'bug']),
    body('content').optional().isLength({ max: 5000 }),
    body('userRole').optional().isIn(['developer', 'user', 'contributor']),
    body('userExperience').optional().isIn(['beginner', 'intermediate', 'expert']),
  ],
  async (req, res) => {
    try {
      const errors = validationResult(req);
      if (!errors.isEmpty()) {
        return res.status(400).json({
          error: 'Validation failed',
          details: errors.array(),
        });
      }

      const feedbackData = {
        ...req.body,
        userAgent: req.get('User-Agent'),
        ipAddress: req.ip,
        sessionId: req.sessionID,
        timestamp: new Date(),
      };

      const feedback = await feedbackService.submitFeedback(feedbackData);

      // Track analytics event
      await analyticsService.trackEvent({
        type: 'feedback_submitted',
        pageId: req.body.pageId,
        userId: req.body.userId,
        sessionId: req.sessionID,
        eventData: { feedbackType: req.body.type },
      });

      res.status(201).json({
        success: true,
        feedbackId: feedback.id,
        message: 'Feedback submitted successfully',
      });
    } catch (error) {
      console.error('Error submitting feedback:', error);
      res.status(500).json({
        error: 'Internal server error',
        message: 'Failed to submit feedback',
      });
    }
  }
);

// Get feedback for a page
router.get('/feedback/:pageId', async (req, res) => {
  try {
    const { pageId } = req.params;
    const feedback = await feedbackService.getFeedbackForPage(pageId);

    res.json({
      success: true,
      feedback: feedback.map(f => ({
        id: f.id,
        type: f.type,
        content: f.content,
        createdAt: f.created_at,
        status: f.status,
      })),
    });
  } catch (error) {
    console.error('Error fetching feedback:', error);
    res.status(500).json({ error: 'Failed to fetch feedback' });
  }
});

// Analytics endpoint
router.post(
  '/analytics/track',
  [
    body('eventType').notEmpty().isLength({ max: 100 }),
    body('pageId').optional().isLength({ max: 255 }),
    body('eventData').optional().isObject(),
  ],
  async (req, res) => {
    try {
      const errors = validationResult(req);
      if (!errors.isEmpty()) {
        return res.status(400).json({ error: 'Validation failed' });
      }

      await analyticsService.trackEvent({
        ...req.body,
        userAgent: req.get('User-Agent'),
        ipAddress: req.ip,
        sessionId: req.sessionID,
        referrer: req.get('Referrer'),
      });

      res.json({ success: true });
    } catch (error) {
      console.error('Error tracking analytics:', error);
      res.status(500).json({ error: 'Failed to track event' });
    }
  }
);

export default router;
```

### Feedback Service Implementation

```typescript
// src/services/FeedbackService.ts
import { dbPool } from '../database/connection';
import { v4 as uuidv4 } from 'uuid';

export interface FeedbackData {
  pageId: string;
  type: 'positive' | 'negative' | 'suggestion' | 'bug';
  content?: string;
  userId?: string;
  userRole?: string;
  userExperience?: string;
  userAgent?: string;
  ipAddress?: string;
  sessionId?: string;
  pageLoadTime?: number;
  previousPages?: string[];
}

export class FeedbackService {
  async submitFeedback(data: FeedbackData) {
    const client = await dbPool.connect();

    try {
      await client.query('BEGIN');

      // Insert feedback entry
      const feedbackQuery = `
        INSERT INTO feedback_entries (
          id, page_id, feedback_type, content, user_id, user_role, 
          user_experience, user_agent, ip_address, session_id,
          page_load_time, previous_pages, priority, category
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        RETURNING id, created_at
      `;

      const priority = this.calculatePriority(data);
      const category = await this.categorize(data);

      const result = await client.query(feedbackQuery, [
        uuidv4(),
        data.pageId,
        data.type,
        data.content,
        data.userId,
        data.userRole,
        data.userExperience,
        data.userAgent,
        data.ipAddress,
        data.sessionId,
        data.pageLoadTime,
        data.previousPages,
        priority,
        category,
      ]);

      const feedback = result.rows[0];

      // Update page statistics
      await this.updatePageStatistics(client, data.pageId, data.type);

      await client.query('COMMIT');

      // Trigger notification for high priority feedback
      if (priority === 'high' || priority === 'urgent') {
        await this.sendHighPriorityNotification(feedback, data);
      }

      return feedback;
    } catch (error) {
      await client.query('ROLLBACK');
      throw error;
    } finally {
      client.release();
    }
  }

  private calculatePriority(data: FeedbackData): string {
    let score = 0;

    // Type scoring
    if (data.type === 'bug') score += 3;
    else if (data.type === 'negative') score += 2;
    else if (data.type === 'suggestion') score += 1;

    // User role scoring
    if (data.userRole === 'developer') score += 2;
    else if (data.userRole === 'contributor') score += 1;

    // Content analysis scoring
    if (data.content) {
      const urgentKeywords = ['broken', 'error', 'crash', 'urgent', 'critical'];
      const hasUrgentKeywords = urgentKeywords.some(keyword =>
        data.content.toLowerCase().includes(keyword)
      );
      if (hasUrgentKeywords) score += 2;
    }

    if (score >= 5) return 'urgent';
    if (score >= 3) return 'high';
    if (score >= 2) return 'medium';
    return 'low';
  }

  private async categorize(data: FeedbackData): Promise<string> {
    // Simple keyword-based categorization
    // In production, this could use ML/AI for better categorization

    if (!data.content) return 'general';

    const content = data.content.toLowerCase();

    const categories = {
      troubleshooting: ['error', 'problem', 'issue', 'broken', 'fix', 'help'],
      onboarding: ['setup', 'getting started', 'installation', 'beginner', 'new'],
      'api-docs': ['api', 'endpoint', 'response', 'parameter', 'authentication'],
      'code-standards': ['style', 'format', 'convention', 'standard', 'guideline'],
      search: ['search', 'find', 'look for', 'locate', 'discovery'],
      navigation: ['navigate', 'menu', 'link', 'structure', 'organization'],
      examples: ['example', 'sample', 'demo', 'tutorial', 'walkthrough'],
    };

    for (const [category, keywords] of Object.entries(categories)) {
      if (keywords.some(keyword => content.includes(keyword))) {
        return category;
      }
    }

    return 'general';
  }

  private async updatePageStatistics(client: any, pageId: string, feedbackType: string) {
    // Update page feedback statistics
    const updateQuery = `
      UPDATE documentation_pages 
      SET 
        view_count = view_count + 1,
        updated_at = NOW()
      WHERE id = $1
    `;

    await client.query(updateQuery, [pageId]);
  }

  private async sendHighPriorityNotification(feedback: any, data: FeedbackData) {
    // Send Slack notification for high priority feedback
    try {
      const slackWebhookUrl = process.env.SLACK_WEBHOOK_URL;
      if (!slackWebhookUrl) return;

      const message = {
        text: `🚨 High Priority Documentation Feedback Received`,
        blocks: [
          {
            type: 'header',
            text: {
              type: 'plain_text',
              text: '🚨 High Priority Documentation Feedback',
            },
          },
          {
            type: 'section',
            fields: [
              { type: 'mrkdwn', text: `*Page:* ${data.pageId}` },
              { type: 'mrkdwn', text: `*Type:* ${data.type}` },
              { type: 'mrkdwn', text: `*User Role:* ${data.userRole || 'Unknown'}` },
              { type: 'mrkdwn', text: `*Priority:* ${feedback.priority}` },
            ],
          },
          {
            type: 'section',
            text: {
              type: 'mrkdwn',
              text: `*Feedback:*\n${data.content || 'No additional details provided'}`,
            },
          },
          {
            type: 'actions',
            elements: [
              {
                type: 'button',
                text: { type: 'plain_text', text: 'View Feedback' },
                url: `${process.env.ADMIN_URL}/feedback/${feedback.id}`,
              },
            ],
          },
        ],
      };

      await fetch(slackWebhookUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(message),
      });
    } catch (error) {
      console.error('Failed to send Slack notification:', error);
    }
  }

  async getFeedbackForPage(pageId: string) {
    const query = `
      SELECT id, feedback_type as type, content, created_at, status, priority
      FROM feedback_entries 
      WHERE page_id = $1 
      ORDER BY created_at DESC 
      LIMIT 100
    `;

    const result = await dbPool.query(query, [pageId]);
    return result.rows;
  }

  async getFeedbackSummary(timeframe: 'daily' | 'weekly' | 'monthly') {
    const timeCondition = {
      daily: "created_at >= NOW() - INTERVAL '1 day'",
      weekly: "created_at >= NOW() - INTERVAL '1 week'",
      monthly: "created_at >= NOW() - INTERVAL '1 month'",
    }[timeframe];

    const query = `
      SELECT 
        COUNT(*) as total_feedback,
        COUNT(CASE WHEN feedback_type = 'positive' THEN 1 END) as positive,
        COUNT(CASE WHEN feedback_type = 'negative' THEN 1 END) as negative,
        COUNT(CASE WHEN feedback_type = 'suggestion' THEN 1 END) as suggestions,
        COUNT(CASE WHEN feedback_type = 'bug' THEN 1 END) as bugs,
        COUNT(CASE WHEN priority = 'urgent' THEN 1 END) as urgent,
        COUNT(CASE WHEN priority = 'high' THEN 1 END) as high_priority,
        COUNT(CASE WHEN status = 'resolved' THEN 1 END) as resolved
      FROM feedback_entries 
      WHERE ${timeCondition}
    `;

    const result = await dbPool.query(query);
    return result.rows[0];
  }
}
```

## 3. Frontend Feedback Widget

### React Feedback Component

```typescript
// src/components/FeedbackWidget.tsx
import React, { useState } from 'react';
import { submitFeedback, trackAnalyticsEvent } from '../services/api';

interface FeedbackWidgetProps {
  pageId: string;
  userId?: string;
  userRole?: 'developer' | 'user' | 'contributor';
}

export const FeedbackWidget: React.FC<FeedbackWidgetProps> = ({
  pageId,
  userId,
  userRole
}) => {
  const [isExpanded, setIsExpanded] = useState(false);
  const [feedback, setFeedback] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [feedbackType, setFeedbackType] = useState<string>('');

  const handleQuickFeedback = async (type: 'positive' | 'negative') => {
    setIsSubmitting(true);

    try {
      await submitFeedback({
        pageId,
        type,
        userId,
        userRole,
        pageLoadTime: performance.now()
      });

      await trackAnalyticsEvent({
        eventType: 'quick_feedback_submitted',
        pageId,
        userId,
        eventData: { feedbackType: type }
      });

      setSubmitted(true);
      setTimeout(() => setSubmitted(false), 3000);

    } catch (error) {
      console.error('Failed to submit feedback:', error);
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDetailedFeedback = async () => {
    if (!feedback.trim() || !feedbackType) return;

    setIsSubmitting(true);

    try {
      await submitFeedback({
        pageId,
        type: feedbackType as any,
        content: feedback,
        userId,
        userRole,
        pageLoadTime: performance.now()
      });

      await trackAnalyticsEvent({
        eventType: 'detailed_feedback_submitted',
        pageId,
        userId,
        eventData: {
          feedbackType,
          contentLength: feedback.length
        }
      });

      setFeedback('');
      setFeedbackType('');
      setIsExpanded(false);
      setSubmitted(true);
      setTimeout(() => setSubmitted(false), 3000);

    } catch (error) {
      console.error('Failed to submit feedback:', error);
    } finally {
      setIsSubmitting(false);
    }
  };

  if (submitted) {
    return (
      <div className="feedback-widget submitted">
        <div className="success-message">
          ✅ Thank you for your feedback!
        </div>
      </div>
    );
  }

  return (
    <div className="feedback-widget">
      <div className="feedback-prompt">
        <h4>Was this documentation helpful?</h4>

        <div className="quick-feedback">
          <button
            onClick={() => handleQuickFeedback('positive')}
            disabled={isSubmitting}
            className="btn-positive"
          >
            👍 Yes
          </button>

          <button
            onClick={() => handleQuickFeedback('negative')}
            disabled={isSubmitting}
            className="btn-negative"
          >
            👎 No
          </button>

          <button
            onClick={() => setIsExpanded(true)}
            className="btn-detailed"
          >
            💬 Leave Feedback
          </button>
        </div>
      </div>

      {isExpanded && (
        <div className="detailed-feedback">
          <div className="feedback-type-selector">
            <label>Feedback Type:</label>
            <select
              value={feedbackType}
              onChange={(e) => setFeedbackType(e.target.value)}
            >
              <option value="">Select type...</option>
              <option value="suggestion">Suggestion</option>
              <option value="bug">Bug Report</option>
              <option value="negative">General Feedback</option>
            </select>
          </div>

          <textarea
            value={feedback}
            onChange={(e) => setFeedback(e.target.value)}
            placeholder="Please share your feedback, suggestions, or report any issues..."
            rows={4}
            maxLength={5000}
          />

          <div className="feedback-actions">
            <button
              onClick={handleDetailedFeedback}
              disabled={isSubmitting || !feedback.trim() || !feedbackType}
              className="btn-submit"
            >
              {isSubmitting ? 'Submitting...' : 'Submit Feedback'}
            </button>

            <button
              onClick={() => setIsExpanded(false)}
              className="btn-cancel"
            >
              Cancel
            </button>
          </div>

          <div className="character-count">
            {feedback.length}/5000 characters
          </div>
        </div>
      )}
    </div>
  );
};
```

### Feedback Widget Styles

```css
/* src/styles/feedback-widget.css */
.feedback-widget {
  background: #f8f9fa;
  border: 1px solid #e9ecef;
  border-radius: 8px;
  padding: 16px;
  margin: 24px 0;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
}

.feedback-prompt h4 {
  margin: 0 0 12px 0;
  font-size: 16px;
  font-weight: 600;
  color: #495057;
}

.quick-feedback {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.quick-feedback button {
  padding: 8px 16px;
  border: none;
  border-radius: 4px;
  font-size: 14px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.btn-positive {
  background: #28a745;
  color: white;
}

.btn-positive:hover {
  background: #218838;
}

.btn-negative {
  background: #dc3545;
  color: white;
}

.btn-negative:hover {
  background: #c82333;
}

.btn-detailed {
  background: #007bff;
  color: white;
}

.btn-detailed:hover {
  background: #0056b3;
}

.detailed-feedback {
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid #e9ecef;
}

.feedback-type-selector {
  margin-bottom: 12px;
}

.feedback-type-selector label {
  display: block;
  margin-bottom: 4px;
  font-weight: 600;
  color: #495057;
}

.feedback-type-selector select {
  width: 100%;
  padding: 8px;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 14px;
}

.detailed-feedback textarea {
  width: 100%;
  padding: 12px;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 14px;
  resize: vertical;
  min-height: 100px;
}

.feedback-actions {
  display: flex;
  gap: 8px;
  margin-top: 12px;
}

.btn-submit {
  background: #28a745;
  color: white;
  padding: 8px 16px;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}

.btn-submit:disabled {
  background: #6c757d;
  cursor: not-allowed;
}

.btn-cancel {
  background: #6c757d;
  color: white;
  padding: 8px 16px;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}

.character-count {
  font-size: 12px;
  color: #6c757d;
  text-align: right;
  margin-top: 4px;
}

.success-message {
  background: #d4edda;
  color: #155724;
  padding: 12px;
  border-radius: 4px;
  text-align: center;
  font-weight: 600;
}

/* Responsive design */
@media (max-width: 768px) {
  .quick-feedback {
    flex-direction: column;
  }

  .quick-feedback button {
    width: 100%;
  }

  .feedback-actions {
    flex-direction: column;
  }

  .feedback-actions button {
    width: 100%;
  }
}
```

## 4. Analytics Service Setup

```typescript
// src/services/AnalyticsService.ts
import { dbPool, redisClient } from '../database/connection';

export interface AnalyticsEvent {
  eventType: string;
  pageId?: string;
  userId?: string;
  sessionId?: string;
  eventData?: Record<string, any>;
  userAgent?: string;
  ipAddress?: string;
  referrer?: string;
}

export class AnalyticsService {
  async trackEvent(event: AnalyticsEvent) {
    try {
      const query = `
        INSERT INTO analytics_events (
          event_type, page_id, user_id, session_id, event_data,
          user_agent, ip_address, referrer
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
      `;

      await dbPool.query(query, [
        event.eventType,
        event.pageId,
        event.userId,
        event.sessionId,
        JSON.stringify(event.eventData || {}),
        event.userAgent,
        event.ipAddress,
        event.referrer,
      ]);

      // Cache event for real-time analytics
      const cacheKey = `analytics:${event.eventType}:${Date.now()}`;
      await redisClient.setEx(cacheKey, 3600, JSON.stringify(event));
    } catch (error) {
      console.error('Failed to track analytics event:', error);
    }
  }

  async getPageAnalytics(pageId: string, timeframe: string) {
    const timeCondition = this.getTimeCondition(timeframe);

    const query = `
      SELECT 
        COUNT(*) as total_events,
        COUNT(CASE WHEN event_type = 'page_view' THEN 1 END) as page_views,
        COUNT(CASE WHEN event_type = 'feedback_submitted' THEN 1 END) as feedback_count,
        AVG(CASE WHEN event_data->>'page_load_time' IS NOT NULL 
            THEN (event_data->>'page_load_time')::float END) as avg_load_time
      FROM analytics_events 
      WHERE page_id = $1 AND ${timeCondition}
    `;

    const result = await dbPool.query(query, [pageId]);
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
        return "created_at >= NOW() - INTERVAL '1 day'";
    }
  }
}
```

## 5. Environment Configuration

```bash
# .env
# Database Configuration
DB_HOST=localhost
DB_PORT=5432
DB_NAME=wrfcoin_docs
DB_USER=postgres
DB_PASSWORD=your_password

# Redis Configuration
REDIS_URL=redis://localhost:6379

# API Configuration
PORT=3001
API_BASE_URL=http://localhost:3001
CORS_ORIGIN=http://localhost:3000

# Notification Configuration
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK
SLACK_BOT_TOKEN=xoxb-your-slack-bot-token

# GitHub Configuration
GITHUB_TOKEN=your_github_token
GITHUB_REPO_OWNER=your-org
GITHUB_REPO_NAME=wrfcoin

# Analytics Configuration
ADMIN_URL=http://localhost:3000/admin
ENABLE_ANALYTICS=true
ANALYTICS_RETENTION_DAYS=90

# Security Configuration
SESSION_SECRET=your_session_secret
RATE_LIMIT_WINDOW_MS=900000
RATE_LIMIT_MAX_REQUESTS=100
```

## 6. Deployment Scripts

```bash
#!/bin/bash
# scripts/deploy-feedback-system.sh

echo "🚀 Deploying WRFCoin Documentation Feedback System"

# Build the application
echo "📦 Building application..."
npm run build

# Run database migrations
echo "🗄️ Running database migrations..."
npm run db:migrate

# Start the services
echo "🌟 Starting feedback API service..."
pm2 start ecosystem.config.js --only feedback-api

# Start analytics processor
echo "📊 Starting analytics processor..."
pm2 start ecosystem.config.js --only analytics-processor

# Health check
echo "🔍 Performing health check..."
sleep 5
curl -f http://localhost:3001/health || exit 1

echo "✅ Feedback system deployed successfully!"
echo "🌐 API available at: http://localhost:3001"
echo "📊 Admin interface at: http://localhost:3000/admin"
```

---

**Next Steps:**

1. [Configure monitoring and alerts](./analytics-setup.md)
2. [Set up GitHub integrations](./github-integration.md)
3. [Train team on feedback workflows](./team-training.md)

**Last Updated**: January 2025  
**Version**: 1.0.0
