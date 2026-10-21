/**
 * Authentication Middleware Tests
 * Tests for JWT and API key authentication with TypeScript types
 */

import { Request, Response, NextFunction } from 'express';
import { AuthenticationManager } from '../../backend/server/middleware/auth';
import {
  AuthenticatedRequest,
  JWTPayload,
  APIKeyValidation,
  AuthenticationResult,
} from '../../backend/shared/types/security-types';
import jwt from 'jsonwebtoken';

describe('Authentication Middleware', () => {
  let authManager: AuthenticationManager;
  let mockReq: Partial<AuthenticatedRequest>;
  let mockRes: Partial<Response>;
  let mockNext: NextFunction;

  const testConfig = {
    jwtSecret: 'test-secret-key',
    jwtExpiresIn: '1h',
    apiKeyHeader: 'X-API-Key',
    bypassAuthInDev: false,
    sessionTimeout: 3600000,
    maxConcurrentSessions: 5,
  };

  beforeEach(() => {
    authManager = new AuthenticationManager(testConfig);

    mockReq = {
      headers: {},
      path: '/api/test',
      method: 'GET',
    };

    mockRes = {
      status: jest.fn().mockReturnThis(),
      json: jest.fn().mockReturnThis(),
      setHeader: jest.fn(),
    };

    mockNext = jest.fn();

    jest.clearAllMocks();
  });

  describe('JWT Authentication', () => {
    const testUser = {
      userId: 'user123',
      roles: ['user'],
      permissions: ['read'],
      sessionId: 'session123',
    };

    it('should authenticate valid JWT token', async () => {
      const token = authManager.generateJWT(testUser);
      mockReq.headers = {
        authorization: `Bearer ${token}`,
      };

      authManager.authenticateJWT(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockNext).toHaveBeenCalled();
      expect(mockReq.user).toEqual(testUser);
      expect(mockRes.status).not.toHaveBeenCalled();
    });

    it('should reject expired JWT token', async () => {
      const expiredToken = jwt.sign(
        { ...testUser, exp: Math.floor(Date.now() / 1000) - 3600 },
        testConfig.jwtSecret
      );

      mockReq.headers = {
        authorization: `Bearer ${expiredToken}`,
      };

      authManager.authenticateJWT(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(401);
      expect(mockNext).not.toHaveBeenCalled();
    });

    it('should reject malformed JWT token', async () => {
      mockReq.headers = {
        authorization: 'Bearer invalid.token.here',
      };

      authManager.authenticateJWT(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(401);
      expect(mockNext).not.toHaveBeenCalled();
    });

    it('should handle missing authorization header', async () => {
      authManager.authenticateJWT(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(401);
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('API Key Authentication', () => {
    const validApiKey = 'test-api-key-123';
    const keyPermissions = ['read', 'write'];

    beforeEach(() => {
      // Mock API key validation
      jest.spyOn(authManager, 'validateApiKey').mockResolvedValue({
        isValid: true,
        permissions: keyPermissions,
        rateLimitTier: 'standard',
      });
    });

    it('should authenticate valid API key', async () => {
      mockReq.get = jest.fn().mockReturnValue(validApiKey);

      await authManager.authenticateApiKey(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockNext).toHaveBeenCalled();
      expect(mockReq.apiKey).toEqual({
        key: validApiKey,
        permissions: keyPermissions,
        rateLimitTier: 'standard',
      });
    });

    it('should reject invalid API key', async () => {
      const invalidKey = 'invalid-key';
      mockReq.get = jest.fn().mockReturnValue(invalidKey);

      jest.spyOn(authManager, 'validateApiKey').mockResolvedValue({
        isValid: false,
        reason: 'Key not found',
      });

      await authManager.authenticateApiKey(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(401);
      expect(mockNext).not.toHaveBeenCalled();
    });

    it('should handle missing API key', async () => {
      mockReq.get = jest.fn().mockReturnValue(undefined);

      await authManager.authenticateApiKey(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(401);
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('Role-Based Authorization', () => {
    const testUser = {
      userId: 'user123',
      roles: ['user', 'moderator'],
      permissions: ['read', 'write'],
      sessionId: 'session123',
    };

    it('should allow access with required role', async () => {
      mockReq.user = testUser;
      const middleware = authManager.requireRole(['user']);

      middleware(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockNext).toHaveBeenCalled();
      expect(mockRes.status).not.toHaveBeenCalled();
    });

    it('should deny access without required role', async () => {
      mockReq.user = testUser;
      const middleware = authManager.requireRole(['admin']);

      middleware(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(403);
      expect(mockNext).not.toHaveBeenCalled();
    });

    it('should handle unauthenticated request', async () => {
      const middleware = authManager.requireRole(['user']);

      middleware(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(401);
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('Permission-Based Authorization', () => {
    const testUser = {
      userId: 'user123',
      roles: ['user'],
      permissions: ['read', 'write', 'delete'],
      sessionId: 'session123',
    };

    it('should allow access with required permission', async () => {
      mockReq.user = testUser;
      const middleware = authManager.requirePermission('read');

      middleware(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockNext).toHaveBeenCalled();
      expect(mockRes.status).not.toHaveBeenCalled();
    });

    it('should deny access without required permission', async () => {
      mockReq.user = testUser;
      const middleware = authManager.requirePermission('admin');

      middleware(
        mockReq as AuthenticatedRequest,
        mockRes as Response,
        mockNext
      );

      expect(mockRes.status).toHaveBeenCalledWith(403);
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('Session Management', () => {
    const testUser = {
      userId: 'user123',
      roles: ['user'],
      permissions: ['read'],
      sessionId: 'session123',
    };

    it('should create and track session', async () => {
      const token = authManager.generateJWT(testUser);

      expect(token).toBeDefined();
      expect(typeof token).toBe('string');

      const sessions = authManager.getUserSessions(testUser.userId);
      expect(sessions).toHaveLength(1);
      expect(sessions[0].sessionId).toBe(testUser.sessionId);
    });

    it('should enforce concurrent session limits', async () => {
      const userId = 'user-limit-test';

      // Create maximum allowed sessions
      for (let i = 0; i < testConfig.maxConcurrentSessions; i++) {
        authManager.generateJWT({
          ...testUser,
          userId,
          sessionId: `session${i}`,
        });
      }

      // Try to create one more session
      const extraToken = authManager.generateJWT({
        ...testUser,
        userId,
        sessionId: 'extra-session',
      });

      const sessions = authManager.getUserSessions(userId);
      expect(sessions).toHaveLength(testConfig.maxConcurrentSessions);
    });

    it('should invalidate expired sessions', async () => {
      const userId = 'user-cleanup-test';
      authManager.generateJWT({ ...testUser, userId });

      // Mock expired session
      const sessions = authManager.getUserSessions(userId);
      sessions[0].expiresAt = new Date(Date.now() - 1000);

      authManager.cleanupExpiredSessions();

      const remainingSessions = authManager.getUserSessions(userId);
      expect(remainingSessions).toHaveLength(0);
    });
  });

  describe('Security Features', () => {
    it('should detect and prevent brute force attacks', async () => {
      const clientIp = '192.168.1.100';
      mockReq.ip = clientIp;

      // Simulate multiple failed attempts
      for (let i = 0; i < 6; i++) {
        mockReq.headers = { authorization: 'Bearer invalid-token' };
        authManager.authenticateJWT(
          mockReq as AuthenticatedRequest,
          mockRes as Response,
          mockNext
        );
      }

      // Next attempt should be blocked
      const isBlocked = authManager.isClientBlocked(clientIp);
      expect(isBlocked).toBe(true);
    });

    it('should implement rate limiting', async () => {
      const clientIp = '192.168.1.101';

      // Rapid requests should trigger rate limiting
      const requests = Array.from({ length: 100 }, () =>
        authManager.checkRateLimit(clientIp, '/api/test')
      );

      const results = await Promise.all(requests);
      const blocked = results.filter(r => !r.allowed);

      expect(blocked.length).toBeGreaterThan(0);
    });
  });

  describe('Type Safety', () => {
    it('should enforce JWTPayload interface', () => {
      const validPayload: JWTPayload = {
        userId: 'user123',
        roles: ['user'],
        permissions: ['read'],
        sessionId: 'session123',
        iat: Math.floor(Date.now() / 1000),
        exp: Math.floor(Date.now() / 1000) + 3600,
      };

      const token = jwt.sign(validPayload, testConfig.jwtSecret);
      expect(token).toBeDefined();
    });

    it('should validate API key validation response', () => {
      const validation: APIKeyValidation = {
        isValid: true,
        permissions: ['read', 'write'],
        rateLimitTier: 'premium',
      };

      expect(validation.isValid).toBe(true);
      expect(validation.permissions).toContain('read');
    });

    it('should type authenticated requests properly', () => {
      const req: AuthenticatedRequest = {
        ...mockReq,
        user: {
          userId: 'user123',
          roles: ['user'],
          permissions: ['read'],
          sessionId: 'session123',
        },
      } as AuthenticatedRequest;

      expect(req.user?.userId).toBe('user123');
      expect(req.user?.roles).toContain('user');
    });
  });
});
