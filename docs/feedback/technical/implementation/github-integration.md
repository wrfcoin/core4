# GitHub Workflow Integration Setup

This guide provides comprehensive instructions for integrating the documentation feedback system
with GitHub workflows for automated issue creation, assignment, and processing.

## Overview

The GitHub integration automates feedback processing through:

- **Automated Issue Creation** from high-priority feedback
- **Intelligent Assignment** based on feedback categories
- **Workflow Automation** for common feedback patterns
- **Progress Tracking** with labels and milestones
- **Team Notifications** via Slack and email

## 1. GitHub App Configuration

### Create GitHub App

```javascript
// scripts/setup-github-app.js
const { Octokit } = require('@octokit/rest');
const { createAppAuth } = require('@octokit/auth-app');

// GitHub App configuration
const GITHUB_APP_CONFIG = {
  appId: process.env.GITHUB_APP_ID,
  privateKey: process.env.GITHUB_PRIVATE_KEY,
  installationId: process.env.GITHUB_INSTALLATION_ID,
  owner: 'wrfcoin',
  repo: 'wrfcoin',
};

class GitHubFeedbackIntegration {
  constructor() {
    this.octokit = new Octokit({
      authStrategy: createAppAuth,
      auth: {
        appId: GITHUB_APP_CONFIG.appId,
        privateKey: GITHUB_APP_CONFIG.privateKey,
        installationId: GITHUB_APP_CONFIG.installationId,
      },
    });
  }

  /**
   * Creates GitHub issue from feedback entry
   * @param {Object} feedback - Feedback data from database
   * @returns {Object} Created issue information
   */
  async createIssueFromFeedback(feedback) {
    const issueTemplate = this.generateIssueTemplate(feedback);
    const labels = this.determineLabels(feedback);
    const assignees = await this.determineAssignees(feedback);

    try {
      const issue = await this.octokit.rest.issues.create({
        owner: GITHUB_APP_CONFIG.owner,
        repo: GITHUB_APP_CONFIG.repo,
        title: issueTemplate.title,
        body: issueTemplate.body,
        labels: labels,
        assignees: assignees,
      });

      // Link feedback to GitHub issue
      await this.linkFeedbackToIssue(feedback.id, issue.data.number);

      return {
        success: true,
        issueNumber: issue.data.number,
        issueUrl: issue.data.html_url,
      };
    } catch (error) {
      console.error('Failed to create GitHub issue:', error);
      throw error;
    }
  }

  /**
   * Generates standardized issue template from feedback
   */
  generateIssueTemplate(feedback) {
    const pageUrl = `https://docs.wrfcoin.org/page/${feedback.page_id}`;
    const priority = feedback.priority.toUpperCase();
    const type = feedback.feedback_type.charAt(0).toUpperCase() + feedback.feedback_type.slice(1);

    const title = `[DOCS] ${type}: ${this.truncateTitle(feedback.content || 'Page feedback')}`;

    const body = `
## Documentation Feedback Issue

**Priority:** ${priority}  
**Type:** ${type}  
**Page:** [${feedback.page_id}](${pageUrl})  
**Submitted:** ${new Date(feedback.created_at).toISOString()}

### User Feedback

${feedback.content || 'No additional details provided'}

### Context Information

- **User Role:** ${feedback.user_role || 'Unknown'}
- **User Experience:** ${feedback.user_experience || 'Unknown'}
- **Session ID:** ${feedback.session_id}
- **Page Load Time:** ${feedback.page_load_time || 'Unknown'}ms
- **Category:** ${feedback.category}

### Technical Details

\`\`\`json
{
  "feedback_id": "${feedback.id}",
  "page_id": "${feedback.page_id}",
  "user_agent": "${feedback.user_agent}",
  "created_at": "${feedback.created_at}",
  "ip_address": "REDACTED"
}
\`\`\`

### Acceptance Criteria

- [ ] Review and understand the feedback
- [ ] Identify specific improvements needed
- [ ] Implement documentation changes
- [ ] Test changes for clarity and accuracy
- [ ] Update related documentation if necessary
- [ ] Verify fix addresses the original concern

### Definition of Done

- [ ] Documentation updated and accurate
- [ ] Changes reviewed by documentation team
- [ ] User experience improved
- [ ] Feedback marked as resolved
- [ ] Analytics show improved user satisfaction

---

*This issue was automatically created from documentation feedback. For questions, contact the documentation team.*
`;

    return { title, body };
  }

  /**
   * Determines appropriate labels for the issue
   */
  determineLabels(feedback) {
    const labels = ['documentation'];

    // Priority labels
    const priorityLabels = {
      urgent: 'priority:urgent',
      high: 'priority:high',
      medium: 'priority:medium',
      low: 'priority:low',
    };
    labels.push(priorityLabels[feedback.priority] || 'priority:medium');

    // Type labels
    const typeLabels = {
      bug: 'type:bug',
      suggestion: 'type:enhancement',
      positive: 'type:positive-feedback',
      negative: 'type:improvement',
    };
    labels.push(typeLabels[feedback.feedback_type] || 'type:feedback');

    // Category labels
    if (feedback.category) {
      labels.push(`category:${feedback.category}`);
    }

    // Special labels for high-impact feedback
    if (feedback.priority === 'urgent' || feedback.priority === 'high') {
      labels.push('needs-immediate-attention');
    }

    return labels;
  }

  /**
   * Determines appropriate assignees based on feedback category
   */
  async determineAssignees(feedback) {
    const categoryAssignments = {
      troubleshooting: ['troubleshooting-team-lead'],
      onboarding: ['onboarding-specialist'],
      'api-docs': ['api-documentation-lead'],
      'code-standards': ['code-standards-maintainer'],
      search: ['ux-lead'],
      navigation: ['ux-lead', 'frontend-lead'],
      examples: ['technical-writer'],
      general: ['documentation-lead'],
    };

    const defaultAssignees = ['documentation-lead'];
    const assignees = categoryAssignments[feedback.category] || defaultAssignees;

    // For urgent issues, also assign team lead
    if (feedback.priority === 'urgent') {
      assignees.push('documentation-team-lead');
    }

    // Remove duplicates and verify assignees exist
    const uniqueAssignees = [...new Set(assignees)];
    return await this.validateAssignees(uniqueAssignees);
  }

  async validateAssignees(assignees) {
    const validAssignees = [];

    for (const assignee of assignees) {
      try {
        await this.octokit.rest.users.getByUsername({ username: assignee });
        validAssignees.push(assignee);
      } catch (error) {
        console.warn(`Assignee ${assignee} not found, skipping`);
      }
    }

    return validAssignees;
  }

  truncateTitle(content, maxLength = 80) {
    if (content.length <= maxLength) return content;
    return content.substring(0, maxLength - 3) + '...';
  }

  async linkFeedbackToIssue(feedbackId, issueNumber) {
    // Update feedback entry with GitHub issue reference
    const query = `
      UPDATE feedback_entries 
      SET github_issue_number = $1, updated_at = NOW()
      WHERE id = $2
    `;

    await dbPool.query(query, [issueNumber, feedbackId]);
  }
}

module.exports = { GitHubFeedbackIntegration };
```

### GitHub App Permissions

Required permissions for the GitHub App:

```yaml
# github-app-permissions.yml
permissions:
  issues: write
  repository_projects: read
  metadata: read
  pull_requests: read

events:
  - issues
  - issue_comment
  - pull_request
  - project_card
```

## 2. Automated Workflow Setup

### Issue Processing Workflow

```yaml
# .github/workflows/feedback-processing.yml
name: Documentation Feedback Processing

on:
  issues:
    types: [opened, labeled, closed]
  issue_comment:
    types: [created]
  schedule:
    - cron: '0 9 * * MON' # Weekly summary every Monday

env:
  SLACK_WEBHOOK: ${{ secrets.SLACK_WEBHOOK_URL }}
  FEEDBACK_API_URL: ${{ secrets.FEEDBACK_API_URL }}

jobs:
  process-feedback-issue:
    if: contains(github.event.issue.labels.*.name, 'documentation')
    runs-on: ubuntu-latest

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '18'
          cache: 'npm'

      - name: Install dependencies
        run: npm ci

      - name: Process feedback issue
        uses: actions/github-script@v7
        with:
          script: |
            const { processDocumentationIssue } = require('./scripts/github-feedback-processor.js');

            const issueData = {
              number: context.issue.number,
              action: context.payload.action,
              issue: context.payload.issue,
              comment: context.payload.comment
            };

            await processDocumentationIssue(github, context, issueData);

      - name: Update feedback database
        if: github.event.action == 'closed'
        run: |
          node scripts/update-feedback-status.js \
            --issue-number="${{ github.event.issue.number }}" \
            --status="resolved" \
            --resolution-notes="${{ github.event.issue.state_reason }}"

      - name: Send Slack notification
        if: contains(github.event.issue.labels.*.name, 'priority:urgent')
        uses: 8398a7/action-slack@v3
        with:
          status: custom
          custom_payload: |
            {
              "text": "🚨 Urgent Documentation Issue",
              "blocks": [
                {
                  "type": "section",
                  "text": {
                    "type": "mrkdwn",
                    "text": "*Urgent documentation issue requires attention*"
                  }
                },
                {
                  "type": "section",
                  "fields": [
                    {
                      "type": "mrkdwn",
                      "text": "*Issue:* <${{ github.event.issue.html_url }}|#${{ github.event.issue.number }}>"
                    },
                    {
                      "type": "mrkdwn",
                      "text": "*Title:* ${{ github.event.issue.title }}"
                    },
                    {
                      "type": "mrkdwn",
                      "text": "*Assignee:* ${{ github.event.issue.assignee.login || 'Unassigned' }}"
                    },
                    {
                      "type": "mrkdwn",
                      "text": "*Labels:* ${{ join(github.event.issue.labels.*.name, ', ') }}"
                    }
                  ]
                }
              ]
            }
        env:
          SLACK_WEBHOOK_URL: ${{ secrets.SLACK_WEBHOOK_URL }}

  weekly-feedback-summary:
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Generate weekly summary
        uses: actions/github-script@v7
        with:
          script: |
            const { generateWeeklySummary } = require('./scripts/weekly-feedback-summary.js');
            await generateWeeklySummary(github, context);

      - name: Create summary issue
        uses: actions/github-script@v7
        with:
          script: |
            const fs = require('fs');
            const summary = fs.readFileSync('weekly-summary.md', 'utf8');

            await github.rest.issues.create({
              owner: context.repo.owner,
              repo: context.repo.repo,
              title: `📊 Weekly Documentation Feedback Summary - ${new Date().toISOString().split('T')[0]}`,
              body: summary,
              labels: ['documentation', 'summary', 'weekly-report']
            });
```

### Automated Assignment Workflow

```yaml
# .github/workflows/auto-assignment.yml
name: Documentation Issue Auto-Assignment

on:
  issues:
    types: [opened]
  issue_comment:
    types: [created]

jobs:
  auto-assign:
    if: contains(github.event.issue.labels.*.name, 'documentation')
    runs-on: ubuntu-latest

    steps:
      - name: Auto-assign based on category
        uses: actions/github-script@v7
        with:
          script: |
            const issue = context.payload.issue;
            const labels = issue.labels.map(label => label.name);

            // Define assignment rules
            const assignmentRules = {
              'category:troubleshooting': ['troubleshooting-specialist'],
              'category:onboarding': ['onboarding-lead'],
              'category:api-docs': ['api-documentation-team'],
              'category:code-standards': ['code-standards-maintainer'],
              'priority:urgent': ['documentation-team-lead'],
              'type:bug': ['technical-writer', 'qa-lead']
            };

            let assignees = new Set();

            // Apply assignment rules
            for (const label of labels) {
              if (assignmentRules[label]) {
                assignmentRules[label].forEach(assignee => assignees.add(assignee));
              }
            }

            // Default assignment if no specific rules match
            if (assignees.size === 0) {
              assignees.add('documentation-lead');
            }

            // Add assignees to issue
            if (assignees.size > 0) {
              await github.rest.issues.addAssignees({
                owner: context.repo.owner,
                repo: context.repo.repo,
                issue_number: issue.number,
                assignees: Array.from(assignees).slice(0, 3) // Max 3 assignees
              });
            }

      - name: Add to project board
        uses: actions/github-script@v7
        with:
          script: |
            const issue = context.payload.issue;
            const projectId = 'PVT_kwDOBP9Mos4AH7Ss'; // Documentation project ID

            // Add issue to documentation project
            await github.graphql(`
              mutation($projectId: ID!, $contentId: ID!) {
                addProjectV2ItemById(input: {projectId: $projectId, contentId: $contentId}) {
                  item {
                    id
                  }
                }
              }
            `, {
              projectId: projectId,
              contentId: issue.node_id
            });

      - name: Set initial status
        uses: actions/github-script@v7
        with:
          script: |
            const issue = context.payload.issue;
            const labels = issue.labels.map(label => label.name);

            // Determine initial status based on priority
            let status = 'Triage';
            if (labels.includes('priority:urgent')) {
              status = 'In Progress';
            } else if (labels.includes('priority:high')) {
              status = 'Ready';
            }

            // Add status label
            await github.rest.issues.addLabels({
              owner: context.repo.owner,
              repo: context.repo.repo,
              issue_number: issue.number,
              labels: [`status:${status.toLowerCase()}`]
            });
```

## 3. Issue Templates

### Bug Report Template

```yaml
# .github/ISSUE_TEMPLATE/documentation-bug.yml
name: Documentation Bug Report
description: Report errors, outdated information, or missing content in documentation
title: '[DOCS] Bug: '
labels: ['documentation', 'type:bug']
assignees: ['documentation-team']

body:
  - type: markdown
    attributes:
      value: |
        Thanks for taking the time to report a documentation issue! This helps us improve the WRFCoin documentation for everyone.

  - type: input
    id: page-url
    attributes:
      label: Documentation Page URL
      description: Provide the full URL of the documentation page with the issue
      placeholder: https://docs.wrfcoin.org/...
    validations:
      required: true

  - type: dropdown
    id: issue-type
    attributes:
      label: Issue Type
      description: What type of documentation issue is this?
      options:
        - Outdated information
        - Missing information
        - Unclear instructions
        - Broken links
        - Technical error
        - Formatting issue
        - Other
    validations:
      required: true

  - type: textarea
    id: description
    attributes:
      label: Description
      description: Clear description of the documentation issue
      placeholder: Describe what's wrong with the documentation and what you expected to find
    validations:
      required: true

  - type: textarea
    id: expected-content
    attributes:
      label: Expected Information
      description: What information should be provided instead?
      placeholder: Describe what the documentation should say or include

  - type: dropdown
    id: impact
    attributes:
      label: Impact Level
      description: How does this issue affect your work?
      options:
        - Blocks development work
        - Causes confusion or delays
        - Minor inconvenience
        - Enhancement opportunity
    validations:
      required: true

  - type: textarea
    id: suggested-fix
    attributes:
      label: Suggested Fix
      description: If you have suggestions for improvement, please share them
      placeholder: How would you improve this documentation?

  - type: input
    id: user-role
    attributes:
      label: User Role
      description: What's your role? (e.g., Frontend Developer, DevOps Engineer, etc.)
      placeholder: Frontend Developer

  - type: dropdown
    id: experience-level
    attributes:
      label: Experience Level
      description: Your experience level with WRFCoin platform
      options:
        - Beginner (new to WRFCoin)
        - Intermediate (some experience)
        - Expert (extensive experience)

  - type: checkboxes
    id: terms
    attributes:
      label: Code of Conduct
      description: By submitting this issue, you agree to follow our Code of Conduct
      options:
        - label: I agree to follow this project's Code of Conduct
          required: true
```

### Feature Request Template

```yaml
# .github/ISSUE_TEMPLATE/documentation-enhancement.yml
name: Documentation Enhancement
description: Suggest improvements or new documentation content
title: '[DOCS] Enhancement: '
labels: ['documentation', 'type:enhancement']
assignees: ['documentation-team']

body:
  - type: markdown
    attributes:
      value: |
        Thank you for suggesting documentation improvements! Your feedback helps make WRFCoin documentation more useful.

  - type: dropdown
    id: enhancement-type
    attributes:
      label: Enhancement Type
      description: What type of documentation enhancement is this?
      options:
        - New documentation section
        - Additional examples
        - Better explanations
        - More detailed guides
        - Video tutorials
        - Interactive demos
        - FAQ additions
        - Other
    validations:
      required: true

  - type: textarea
    id: description
    attributes:
      label: Enhancement Description
      description: Describe the documentation enhancement you'd like to see
      placeholder: What documentation would be helpful?
    validations:
      required: true

  - type: textarea
    id: use-case
    attributes:
      label: Use Case
      description: Describe the specific use case or problem this would solve
      placeholder: When would this documentation be useful? What problem does it solve?

  - type: textarea
    id: target-audience
    attributes:
      label: Target Audience
      description: Who would benefit from this documentation?
      placeholder: New developers, experienced users, specific roles, etc.

  - type: dropdown
    id: priority
    attributes:
      label: Priority
      description: How important is this enhancement?
      options:
        - Nice to have
        - Would be helpful
        - Important for workflow
        - Critical gap
    validations:
      required: true

  - type: textarea
    id: content-outline
    attributes:
      label: Content Outline
      description: If you have ideas about the structure or content, please share
      placeholder: What sections, topics, or examples should be included?

  - type: input
    id: related-pages
    attributes:
      label: Related Pages
      description: URLs of related documentation pages
      placeholder: https://docs.wrfcoin.org/...
```

## 4. Project Board Automation

### Project Board Configuration

```javascript
// scripts/setup-project-board.js
const { Octokit } = require('@octokit/rest');

class ProjectBoardManager {
  constructor(octokit) {
    this.octokit = octokit;
    this.projectId = process.env.GITHUB_PROJECT_ID;
  }

  async setupDocumentationProject() {
    // Create project columns
    const columns = [
      { name: 'Triage', purpose: 'New feedback awaiting review' },
      { name: 'Ready', purpose: 'Approved for work' },
      { name: 'In Progress', purpose: 'Currently being worked on' },
      { name: 'Review', purpose: 'Awaiting review' },
      { name: 'Done', purpose: 'Completed and verified' },
    ];

    for (const column of columns) {
      await this.createProjectColumn(column);
    }

    // Setup automation rules
    await this.setupAutomationRules();
  }

  async createProjectColumn(column) {
    try {
      await this.octokit.rest.projects.createColumn({
        project_id: this.projectId,
        name: column.name,
      });
      console.log(`Created column: ${column.name}`);
    } catch (error) {
      console.error(`Failed to create column ${column.name}:`, error);
    }
  }

  async setupAutomationRules() {
    // Move cards automatically based on issue state
    const automationRules = [
      {
        trigger: 'issue.opened',
        action: 'move_to_column',
        column: 'Triage',
      },
      {
        trigger: 'issue.assigned',
        action: 'move_to_column',
        column: 'Ready',
      },
      {
        trigger: 'issue.labeled',
        label: 'status:in-progress',
        action: 'move_to_column',
        column: 'In Progress',
      },
      {
        trigger: 'issue.labeled',
        label: 'status:review',
        action: 'move_to_column',
        column: 'Review',
      },
      {
        trigger: 'issue.closed',
        action: 'move_to_column',
        column: 'Done',
      },
    ];

    console.log('Automation rules configured:', automationRules);
  }

  async moveIssueToColumn(issueNumber, columnName) {
    try {
      // Get project cards for the issue
      const { data: cards } = await this.octokit.rest.projects.listCards({
        column_id: await this.getColumnId(columnName),
      });

      const issueCard = cards.find(
        card => card.content_url && card.content_url.includes(`issues/${issueNumber}`)
      );

      if (issueCard) {
        await this.octokit.rest.projects.moveCard({
          card_id: issueCard.id,
          position: 'top',
          column_id: await this.getColumnId(columnName),
        });
      }
    } catch (error) {
      console.error(`Failed to move issue ${issueNumber} to ${columnName}:`, error);
    }
  }

  async getColumnId(columnName) {
    const { data: columns } = await this.octokit.rest.projects.listColumns({
      project_id: this.projectId,
    });

    const column = columns.find(col => col.name === columnName);
    return column ? column.id : null;
  }
}

module.exports = { ProjectBoardManager };
```

## 5. Slack Integration

### Slack Notification Service

```javascript
// src/services/SlackNotificationService.js
const { WebClient } = require('@slack/web-api');

class SlackNotificationService {
  constructor() {
    this.slack = new WebClient(process.env.SLACK_BOT_TOKEN);
    this.channels = {
      urgent: '#docs-urgent',
      general: '#docs-feedback',
      team: '#documentation-team',
    };
  }

  async notifyUrgentFeedback(feedback, githubIssue) {
    const message = {
      channel: this.channels.urgent,
      text: '🚨 Urgent Documentation Feedback',
      blocks: [
        {
          type: 'header',
          text: {
            type: 'plain_text',
            text: '🚨 Urgent Documentation Feedback',
          },
        },
        {
          type: 'section',
          fields: [
            {
              type: 'mrkdwn',
              text: `*Page:* ${feedback.page_id}`,
            },
            {
              type: 'mrkdwn',
              text: `*Type:* ${feedback.feedback_type}`,
            },
            {
              type: 'mrkdwn',
              text: `*Priority:* ${feedback.priority}`,
            },
            {
              type: 'mrkdwn',
              text: `*Category:* ${feedback.category}`,
            },
          ],
        },
        {
          type: 'section',
          text: {
            type: 'mrkdwn',
            text: `*Feedback:*\n${feedback.content || 'No additional details'}`,
          },
        },
        {
          type: 'actions',
          elements: [
            {
              type: 'button',
              text: {
                type: 'plain_text',
                text: 'View GitHub Issue',
              },
              url: githubIssue.html_url,
              style: 'primary',
            },
            {
              type: 'button',
              text: {
                type: 'plain_text',
                text: 'View Page',
              },
              url: `https://docs.wrfcoin.org/page/${feedback.page_id}`,
            },
          ],
        },
      ],
    };

    await this.slack.chat.postMessage(message);
  }

  async notifyWeeklySummary(summary) {
    const message = {
      channel: this.channels.team,
      text: '📊 Weekly Documentation Feedback Summary',
      blocks: [
        {
          type: 'header',
          text: {
            type: 'plain_text',
            text: '📊 Weekly Documentation Feedback Summary',
          },
        },
        {
          type: 'section',
          fields: [
            {
              type: 'mrkdwn',
              text: `*Total Feedback:* ${summary.totalFeedback}`,
            },
            {
              type: 'mrkdwn',
              text: `*Issues Created:* ${summary.issuesCreated}`,
            },
            {
              type: 'mrkdwn',
              text: `*Issues Resolved:* ${summary.issuesResolved}`,
            },
            {
              type: 'mrkdwn',
              text: `*Satisfaction Score:* ${summary.satisfactionScore}/5.0`,
            },
          ],
        },
        {
          type: 'section',
          text: {
            type: 'mrkdwn',
            text: `*Top Issues:*\n${summary.topIssues
              .map(issue => `• <${issue.url}|${issue.title}>`)
              .join('\n')}`,
          },
        },
      ],
    };

    await this.slack.chat.postMessage(message);
  }

  async notifyIssueResolved(issue, feedback) {
    const message = {
      channel: this.channels.general,
      text: '✅ Documentation Issue Resolved',
      blocks: [
        {
          type: 'section',
          text: {
            type: 'mrkdwn',
            text: `✅ *Documentation issue resolved:* <${issue.html_url}|${issue.title}>`,
          },
        },
        {
          type: 'context',
          elements: [
            {
              type: 'mrkdwn',
              text: `Resolution time: ${this.calculateResolutionTime(feedback.created_at, issue.closed_at)}`,
            },
          ],
        },
      ],
    };

    await this.slack.chat.postMessage(message);
  }

  calculateResolutionTime(createdAt, closedAt) {
    const created = new Date(createdAt);
    const closed = new Date(closedAt);
    const diffMs = closed - created;
    const diffHours = Math.round(diffMs / (1000 * 60 * 60));

    if (diffHours < 24) {
      return `${diffHours} hours`;
    } else {
      const diffDays = Math.round(diffHours / 24);
      return `${diffDays} days`;
    }
  }
}

module.exports = { SlackNotificationService };
```

## 6. Metrics and Reporting

### GitHub Analytics Service

```javascript
// src/services/GitHubAnalyticsService.js
class GitHubAnalyticsService {
  constructor(octokit) {
    this.octokit = octokit;
  }

  async generateFeedbackReport(timeframe = 'week') {
    const since = this.getTimeframeSince(timeframe);

    // Get all documentation issues in timeframe
    const { data: issues } = await this.octokit.rest.issues.listForRepo({
      owner: 'wrfcoin',
      repo: 'wrfcoin',
      labels: 'documentation',
      state: 'all',
      since: since,
    });

    const report = {
      timeframe,
      period: {
        start: since,
        end: new Date().toISOString(),
      },
      metrics: {
        totalIssues: issues.length,
        openIssues: issues.filter(issue => issue.state === 'open').length,
        closedIssues: issues.filter(issue => issue.state === 'closed').length,
        avgResolutionTime: await this.calculateAvgResolutionTime(
          issues.filter(issue => issue.state === 'closed')
        ),
        issuesByPriority: this.groupIssuesByLabel(issues, 'priority:'),
        issuesByCategory: this.groupIssuesByLabel(issues, 'category:'),
        issuesByType: this.groupIssuesByLabel(issues, 'type:'),
      },
      topContributors: await this.getTopContributors(issues),
      mostActivePages: this.getMostActivePages(issues),
      trends: await this.calculateTrends(issues),
    };

    return report;
  }

  async calculateAvgResolutionTime(closedIssues) {
    if (closedIssues.length === 0) return 0;

    const resolutionTimes = closedIssues.map(issue => {
      const created = new Date(issue.created_at);
      const closed = new Date(issue.closed_at);
      return closed - created;
    });

    const avgMs = resolutionTimes.reduce((sum, time) => sum + time, 0) / resolutionTimes.length;
    return Math.round(avgMs / (1000 * 60 * 60)); // Convert to hours
  }

  groupIssuesByLabel(issues, labelPrefix) {
    const groups = {};

    issues.forEach(issue => {
      const matchingLabel = issue.labels.find(label => label.name.startsWith(labelPrefix));

      if (matchingLabel) {
        const key = matchingLabel.name.replace(labelPrefix, '');
        groups[key] = (groups[key] || 0) + 1;
      }
    });

    return groups;
  }

  getMostActivePages(issues) {
    const pageCounts = {};

    issues.forEach(issue => {
      // Extract page reference from issue body
      const pageMatch = issue.body.match(/\*\*Page:\*\* \[([^\]]+)\]/);
      if (pageMatch) {
        const page = pageMatch[1];
        pageCounts[page] = (pageCounts[page] || 0) + 1;
      }
    });

    return Object.entries(pageCounts)
      .sort(([, a], [, b]) => b - a)
      .slice(0, 10)
      .map(([page, count]) => ({ page, count }));
  }

  async getTopContributors(issues) {
    const contributors = {};

    // Count issue creators
    issues.forEach(issue => {
      const user = issue.user.login;
      contributors[user] = contributors[user] || { created: 0, commented: 0, resolved: 0 };
      contributors[user].created++;

      if (issue.state === 'closed' && issue.assignee && issue.assignee.login) {
        const assignee = issue.assignee.login;
        contributors[assignee] = contributors[assignee] || {
          created: 0,
          commented: 0,
          resolved: 0,
        };
        contributors[assignee].resolved++;
      }
    });

    // Get comment contributors
    for (const issue of issues) {
      try {
        const { data: comments } = await this.octokit.rest.issues.listComments({
          owner: 'wrfcoin',
          repo: 'wrfcoin',
          issue_number: issue.number,
        });

        comments.forEach(comment => {
          const user = comment.user.login;
          contributors[user] = contributors[user] || { created: 0, commented: 0, resolved: 0 };
          contributors[user].commented++;
        });
      } catch (error) {
        console.error(`Failed to get comments for issue ${issue.number}:`, error);
      }
    }

    return Object.entries(contributors)
      .map(([user, stats]) => ({ user, ...stats }))
      .sort((a, b) => b.created + b.commented + b.resolved - (a.created + a.commented + a.resolved))
      .slice(0, 10);
  }

  getTimeframeSince(timeframe) {
    const now = new Date();
    const timeframes = {
      day: 1,
      week: 7,
      month: 30,
      quarter: 90,
      year: 365,
    };

    const days = timeframes[timeframe] || 7;
    const since = new Date(now - days * 24 * 60 * 60 * 1000);
    return since.toISOString();
  }
}

module.exports = { GitHubAnalyticsService };
```

## 7. Deployment Configuration

### Environment Variables

```bash
# .env.github-integration
# GitHub App Configuration
GITHUB_APP_ID=123456
GITHUB_PRIVATE_KEY="-----BEGIN RSA PRIVATE KEY-----\n..."
GITHUB_INSTALLATION_ID=12345678
GITHUB_PROJECT_ID=PVT_kwDOBP9Mos4AH7Ss

# Repository Configuration
GITHUB_OWNER=wrfcoin
GITHUB_REPO=wrfcoin

# Slack Integration
SLACK_BOT_TOKEN=xoxb-your-slack-bot-token
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK

# API Configuration
FEEDBACK_API_URL=http://localhost:3001
DOCS_BASE_URL=https://docs.wrfcoin.org

# Feature Flags
ENABLE_AUTO_ASSIGNMENT=true
ENABLE_SLACK_NOTIFICATIONS=true
ENABLE_PROJECT_BOARD_AUTOMATION=true
AUTO_CREATE_ISSUES_FOR_URGENT=true
AUTO_CREATE_ISSUES_FOR_HIGH=false
```

### Deployment Script

```bash
#!/bin/bash
# scripts/deploy-github-integration.sh

echo "🚀 Deploying GitHub Integration for Documentation Feedback"

# Validate environment variables
required_vars=(
    "GITHUB_APP_ID"
    "GITHUB_PRIVATE_KEY"
    "GITHUB_INSTALLATION_ID"
    "SLACK_BOT_TOKEN"
)

for var in "${required_vars[@]}"; do
    if [ -z "${!var}" ]; then
        echo "❌ Missing required environment variable: $var"
        exit 1
    fi
done

# Setup GitHub App
echo "🔧 Configuring GitHub App..."
node scripts/setup-github-app.js

# Create issue templates
echo "📝 Setting up issue templates..."
mkdir -p .github/ISSUE_TEMPLATE
cp templates/github/*.yml .github/ISSUE_TEMPLATE/

# Setup project board
echo "📊 Configuring project board..."
node scripts/setup-project-board.js

# Deploy workflows
echo "⚙️ Deploying GitHub workflows..."
mkdir -p .github/workflows
cp workflows/*.yml .github/workflows/

# Test integration
echo "🧪 Testing GitHub integration..."
node scripts/test-github-integration.js

# Verify Slack notifications
echo "💬 Testing Slack notifications..."
node scripts/test-slack-integration.js

echo "✅ GitHub integration deployed successfully!"
echo ""
echo "Next steps:"
echo "1. Configure team members and permissions"
echo "2. Test feedback submission workflow"
echo "3. Monitor issue creation and assignment"
echo "4. Review and adjust automation rules"
```

## 8. Testing and Validation

### Integration Tests

```javascript
// tests/github-integration.test.js
const { GitHubFeedbackIntegration } = require('../src/services/GitHubFeedbackIntegration');
const { SlackNotificationService } = require('../src/services/SlackNotificationService');

describe('GitHub Feedback Integration', () => {
  let github;
  let slack;

  beforeEach(() => {
    github = new GitHubFeedbackIntegration();
    slack = new SlackNotificationService();
  });

  test('should create GitHub issue from feedback', async () => {
    const mockFeedback = {
      id: 'feedback-123',
      page_id: 'troubleshooting-docker',
      feedback_type: 'bug',
      content: 'The Docker setup instructions are outdated',
      priority: 'high',
      category: 'troubleshooting',
      user_role: 'developer',
      created_at: new Date().toISOString(),
    };

    const result = await github.createIssueFromFeedback(mockFeedback);

    expect(result.success).toBe(true);
    expect(result.issueNumber).toBeDefined();
    expect(result.issueUrl).toContain('github.com');
  });

  test('should assign appropriate labels', () => {
    const feedback = {
      feedback_type: 'bug',
      priority: 'urgent',
      category: 'troubleshooting',
    };

    const labels = github.determineLabels(feedback);

    expect(labels).toContain('documentation');
    expect(labels).toContain('type:bug');
    expect(labels).toContain('priority:urgent');
    expect(labels).toContain('category:troubleshooting');
  });

  test('should send Slack notification for urgent feedback', async () => {
    const mockFeedback = {
      page_id: 'api-docs',
      feedback_type: 'bug',
      priority: 'urgent',
      content: 'API endpoint is broken',
    };

    const mockIssue = {
      html_url: 'https://github.com/wrfcoin/wrfcoin/issues/123',
    };

    // Mock Slack API
    slack.slack.chat.postMessage = jest.fn().mockResolvedValue({ ok: true });

    await slack.notifyUrgentFeedback(mockFeedback, mockIssue);

    expect(slack.slack.chat.postMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        channel: '#docs-urgent',
        text: '🚨 Urgent Documentation Feedback',
      })
    );
  });
});
```

---

**Next Steps:**

1. [Configure team training materials](./team-training.md)
2. [Launch community contribution program](./community-program.md)
3. [Set up monitoring and alerting](./monitoring-setup.md)

**Last Updated**: January 2025  
**Version**: 1.0.0
