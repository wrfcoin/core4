/**
 * Security Alert Middleware Tests
 * Tests for real-time threat detection and monitoring with TypeScript types
 */

import { describe, it, expect, beforeEach, jest } from '@jest/globals';
import { Request, Response, NextFunction } from 'express';
import { SecurityAlertMiddleware } from '../../backend/server/middleware/securityAlertMiddleware';
import {
  SecurityAlert,
  ThreatLevel,
} from '../../backend/shared/types/security-types';

describe('Security Alert Middleware', () => {
  let securityMiddleware: SecurityAlertMiddleware;
  let mockReq: any;
  let mockRes: any;
  let mockNext: jest.Mock;

  beforeEach(() => {
    securityMiddleware = new SecurityAlertMiddleware();

    mockReq = {
      ip: '192.168.1.100',
      path: '/api/test',
      method: 'GET',
      headers: {
        'user-agent': 'Mozilla/5.0 Test Browser',
      },
      body: {},
      query: {},
      param: jest.fn(),
      get: jest.fn().mockReturnValue('Mozilla/5.0 Test Browser'),
      url: '/api/test',
      connection: { remoteAddress: '192.168.1.100' },
    };

    mockRes = {
      status: jest.fn().mockReturnThis(),
      json: jest.fn().mockReturnThis(),
      setHeader: jest.fn(),
    };

    mockNext = jest.fn();

    jest.clearAllMocks();
  });

  describe('Rate Limiting', () => {
    it('should allow requests within rate limit', async () => {
      // Make requests within limit
      for (let i = 0; i < 50; i++) {
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
      }

      expect(mockNext).toHaveBeenCalledTimes(50);
      expect(mockRes.status).not.toHaveBeenCalledWith(429);
    });

    it('should block requests exceeding rate limit', async () => {
      // Exceed rate limit
      for (let i = 0; i < 105; i++) {
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
      }

      expect(mockRes.status).toHaveBeenCalledWith(429);
      expect(mockRes.json).toHaveBeenCalledWith(
        expect.objectContaining({
          error: 'Rate limit exceeded',
          retryAfter: expect.any(Number),
        })
      );
    });

    it('should reset rate limit after window expires', async () => {
      // Fill up rate limit
      for (let i = 0; i < 100; i++) {
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
      }

      // Mock time advance
      jest.advanceTimersByTime(61000); // Advance past window

      // Should allow requests again
      mockNext.mockClear();
      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      expect(mockNext).toHaveBeenCalled();
    });

    it('should track different IPs separately', async () => {
      const ip1 = '192.168.1.100';
      const ip2 = '192.168.1.101';

      // Fill rate limit for IP1
      mockReq.ip = ip1;
      for (let i = 0; i < 100; i++) {
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
      }

      // IP2 should still be allowed
      mockReq.ip = ip2;
      mockNext.mockClear();
      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      expect(mockNext).toHaveBeenCalled();
    });
  });

  describe('Brute Force Protection', () => {
    it('should detect brute force login attempts', async () => {
      mockReq.path = '/api/auth/login';
      mockReq.method = 'POST';

      // Simulate failed login attempts
      for (let i = 0; i < 6; i++) {
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
        securityMiddleware.recordFailedLogin(mockReq.ip);
      }

      // Next attempt should be blocked
      mockRes.status = jest.fn().mockReturnThis();
      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      expect(mockRes.status).toHaveBeenCalledWith(429);
    });

    it('should reset brute force counter after successful login', async () => {
      mockReq.path = '/api/auth/login';

      // Record some failed attempts
      for (let i = 0; i < 3; i++) {
        securityMiddleware.recordFailedLogin(mockReq.ip);
      }

      // Record successful login
      securityMiddleware.recordSuccessfulLogin(mockReq.ip);

      // Counter should be reset
      const isBlocked = securityMiddleware.isBruteForceBlocked(mockReq.ip);
      expect(isBlocked).toBe(false);
    });

    it('should maintain separate counters per IP', async () => {
      const ip1 = '192.168.1.100';
      const ip2 = '192.168.1.101';

      // Block IP1
      for (let i = 0; i < 6; i++) {
        securityMiddleware.recordFailedLogin(ip1);
      }

      // IP2 should not be affected
      const ip2Blocked = securityMiddleware.isBruteForceBlocked(ip2);
      expect(ip2Blocked).toBe(false);
    });
  });

  describe('Suspicious Pattern Detection', () => {
    it('should detect path traversal attempts', async () => {
      mockReq.path = '/api/files/../../../etc/passwd';

      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      expect(mockRes.status).toHaveBeenCalledWith(403);
      expect(mockRes.json).toHaveBeenCalledWith(
        expect.objectContaining({
          error: 'Suspicious request pattern detected',
        })
      );
    });

    it('should detect XSS attempts in query parameters', async () => {
      mockReq.query = {
        search: '<script>alert("xss")</script>',
      };

      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      expect(mockRes.status).toHaveBeenCalledWith(403);
    });

    it('should detect SQL injection in request body', async () => {
      mockReq.body = {
        username: "admin' UNION SELECT * FROM users --",
      };

      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      expect(mockRes.status).toHaveBeenCalledWith(403);
    });

    it('should allow legitimate requests', async () => {
      mockReq.path = '/api/users/profile';
      mockReq.query = { id: '123' };
      mockReq.body = { name: 'John Doe' };

      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      expect(mockNext).toHaveBeenCalled();
      expect(mockRes.status).not.toHaveBeenCalledWith(403);
    });
  });

  describe('Security Alert Generation', () => {
    it('should generate alert for rate limit violations', async () => {
      // Exceed rate limit to trigger alert
      for (let i = 0; i < 105; i++) {
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
      }

      const alerts = securityMiddleware.getActiveAlerts();
      const rateLimitAlert = alerts.find(
        alert => alert.type === 'rate_limit_violation'
      );

      expect(rateLimitAlert).toBeDefined();
      expect(rateLimitAlert?.severity).toBe('medium');
      expect(rateLimitAlert?.clientIp).toBe(mockReq.ip);
    });

    it('should generate alert for brute force attacks', async () => {
      // Trigger brute force protection
      for (let i = 0; i < 6; i++) {
        securityMiddleware.recordFailedLogin(mockReq.ip);
      }

      const alerts = securityMiddleware.getActiveAlerts();
      const bruteForceAlert = alerts.find(
        alert => alert.type === 'brute_force_attack'
      );

      expect(bruteForceAlert).toBeDefined();
      expect(bruteForceAlert?.severity).toBe('high');
    });

    it('should generate alert for suspicious requests', async () => {
      mockReq.path = '/api/admin/../../../etc/passwd';

      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      const alerts = securityMiddleware.getActiveAlerts();
      const suspiciousAlert = alerts.find(
        alert => alert.type === 'suspicious_request'
      );

      expect(suspiciousAlert).toBeDefined();
      expect(suspiciousAlert?.severity).toBe('high');
    });

    it('should allow resolving alerts', async () => {
      // Generate an alert
      mockReq.path = '/api/test/../sensitive';
      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      const alerts = securityMiddleware.getActiveAlerts();
      const alert = alerts[0];

      // Resolve the alert
      securityMiddleware.resolveAlert(alert.id);

      const updatedAlert = securityMiddleware.getAlert(alert.id);
      expect(updatedAlert?.resolved).toBe(true);
      expect(updatedAlert?.resolvedAt).toBeInstanceOf(Date);
    });
  });

  describe('Audit Logging', () => {
    it('should log blocked requests', async () => {
      mockReq.path = '/api/test/../sensitive';

      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      const auditLogs = securityMiddleware.getAuditLogs();
      const blockLog = auditLogs.find(log => log.event === 'REQUEST_BLOCKED');

      expect(blockLog).toBeDefined();
      expect(blockLog?.success).toBe(false);
      expect(blockLog?.blockReason).toContain('Suspicious pattern');
    });

    it('should log rate limit violations', async () => {
      // Trigger rate limit
      for (let i = 0; i < 105; i++) {
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
      }

      const auditLogs = securityMiddleware.getAuditLogs();
      const rateLimitLog = auditLogs.find(
        log =>
          log.event === 'REQUEST_BLOCKED' &&
          log.blockReason?.includes('Rate limit')
      );

      expect(rateLimitLog).toBeDefined();
    });

    it('should log processed requests', async () => {
      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      const auditLogs = securityMiddleware.getAuditLogs();
      const processLog = auditLogs.find(
        log => log.event === 'REQUEST_PROCESSED'
      );

      expect(processLog).toBeDefined();
      expect(processLog?.success).toBe(true);
      expect(processLog?.requestPath).toBe(mockReq.path);
      expect(processLog?.requestMethod).toBe(mockReq.method);
    });
  });

  describe('Performance and Cleanup', () => {
    it('should clean up old tracking data', async () => {
      // Create some old tracking data
      const oldIp = '192.168.1.200';
      securityMiddleware.recordFailedLogin(oldIp);

      // Mock time advance beyond cleanup threshold
      jest.advanceTimersByTime(400000); // 6+ minutes

      securityMiddleware.cleanup();

      // Old data should be cleaned up
      const isBlocked = securityMiddleware.isBruteForceBlocked(oldIp);
      expect(isBlocked).toBe(false);
    });

    it('should handle high request volumes efficiently', async () => {
      const startTime = Date.now();

      // Process many requests
      const requests = Array.from({ length: 1000 }, (_, i) => ({
        ...mockReq,
        ip: `192.168.1.${i % 255}`,
      }));

      for (const req of requests) {
        await securityMiddleware.checkRequest(req, mockRes, mockNext);
      }

      const processingTime = Date.now() - startTime;
      expect(processingTime).toBeLessThan(5000); // Should complete within 5 seconds
    });
  });

  describe('Type Safety', () => {
    it('should enforce SecurityAlert interface', () => {
      const alert: SecurityAlert = {
        id: 'alert-123',
        type: 'suspicious_request',
        severity: 'high' as ThreatLevel,
        message: 'Path traversal detected',
        description: 'Path traversal detected',
        timestamp: new Date(),
        clientIp: '192.168.1.100',
        resolved: false,
      };

      expect(alert.type).toBe('suspicious_request');
      expect(alert.severity).toBe('high');
      expect(alert.timestamp).toBeInstanceOf(Date);
    });

    it('should validate threat levels', () => {
      const validLevels: ThreatLevel[] = ['low', 'medium', 'high', 'critical'];

      validLevels.forEach(level => {
        const alert: SecurityAlert = {
          id: 'test',
          type: 'suspicious_request',
          severity: level,
          message: 'Test alert',
          description: 'Test alert',
          timestamp: new Date(),
          resolved: false,
        };

        expect(alert.severity).toBe(level);
      });
    });
  });
});
