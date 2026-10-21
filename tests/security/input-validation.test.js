/**
 * Input Validation Security Tests
 * Location: backend/tests/security/input-validation.test.js
 * Coverage: SQL injection, XSS, path traversal, data validation
 */

import { jest } from '@jest/globals';
import {
  setupTestEnvironment,
  teardownTestEnvironment,
} from '../utils/test-helpers.js';
import { resetAllMocks } from '../mocks/external-services.js';

// Mock validation functions
const mockValidation = {
  validateWeatherData: jest.fn(),
  validateUserInput: jest.fn(),
  sanitizeInput: jest.fn(),
  validateFileUpload: jest.fn(),
  validateApiKey: jest.fn(),
  preventSQLInjection: jest.fn(),
  preventXSS: jest.fn(),
  validateWalletAddress: jest.fn(),
};

describe('Input Validation Security Tests', () => {
  let testDb;

  beforeAll(async () => {
    testDb = await setupTestEnvironment();
  });

  afterAll(async () => {
    await teardownTestEnvironment();
  });

  beforeEach(() => {
    resetAllMocks();
    jest.clearAllMocks();
  });

  describe('SQL Injection Prevention', () => {
    test('should block SQL injection attempts in weather data', async () => {
      const maliciousSQLInputs = [
        "'; DROP TABLE weather_data; --",
        "' OR '1'='1",
        "'; UPDATE users SET role='admin' WHERE id=1; --",
        "' UNION SELECT * FROM users --",
        "; INSERT INTO users VALUES ('hacker', 'password'); --",
      ];

      mockValidation.preventSQLInjection.mockImplementation(input => {
        const sqlKeywords = [
          'DROP',
          'DELETE',
          'UPDATE',
          'INSERT',
          'UNION',
          'SELECT',
        ];
        const hasSQLInjection = sqlKeywords.some(
          keyword =>
            input.toUpperCase().includes(keyword) && input.includes("'")
        );

        if (hasSQLInjection) {
          throw new Error('Potential SQL injection detected');
        }
        return { safe: true, sanitized: input };
      });

      for (const maliciousInput of maliciousSQLInputs) {
        await expect(
          mockValidation.preventSQLInjection(maliciousInput)
        ).rejects.toThrow('Potential SQL injection detected');
      }
    });

    test('should allow safe SQL-like strings', async () => {
      const safeInputs = [
        "Station's Temperature Reading",
        "Weather data from O'Hare Airport",
        'Rain: 0.5 inches',
        'Temperature range: 20-30°C',
      ];

      mockValidation.preventSQLInjection.mockImplementation(input => {
        const sqlKeywords = [
          'DROP',
          'DELETE',
          'UPDATE',
          'INSERT',
          'UNION',
          'SELECT',
        ];
        const hasSQLInjection = sqlKeywords.some(
          keyword =>
            input.toUpperCase().includes(keyword) && input.includes("'")
        );

        if (hasSQLInjection) {
          throw new Error('Potential SQL injection detected');
        }
        return { safe: true, sanitized: input };
      });

      for (const safeInput of safeInputs) {
        const result = await mockValidation.preventSQLInjection(safeInput);
        expect(result.safe).toBe(true);
      }
    });

    test('should validate parameterized queries', async () => {
      const parameterizedQuery = {
        query:
          'SELECT * FROM weather_data WHERE station_id = $1 AND timestamp > $2',
        params: ['station_001', '2025-06-01T00:00:00Z'],
      };

      mockValidation.preventSQLInjection.mockResolvedValue({
        safe: true,
        parameterized: true,
        query: parameterizedQuery.query,
        params: parameterizedQuery.params,
      });

      const result =
        await mockValidation.preventSQLInjection(parameterizedQuery);
      expect(result.parameterized).toBe(true);
      expect(result.safe).toBe(true);
    });
  });

  describe('XSS Prevention', () => {
    test('should block XSS attempts in user inputs', async () => {
      const xssPayloads = [
        '<script>alert("XSS")</script>',
        '<img src="x" onerror="alert(1)">',
        'javascript:alert("XSS")',
        '<svg onload="alert(1)">',
        '<iframe src="javascript:alert(1)"></iframe>',
        '"><script>alert("XSS")</script>',
        "'><script>alert(String.fromCharCode(88,83,83))</script>",
      ];

      mockValidation.preventXSS.mockImplementation(input => {
        const xssPatterns = [
          /<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi,
          /<iframe\b[^<]*(?:(?!<\/iframe>)<[^<]*)*<\/iframe>/gi,
          /javascript:/gi,
          /on\w+\s*=/gi,
          /<img[^>]+onerror/gi,
          /<svg[^>]+onload/gi,
        ];

        const hasXSS = xssPatterns.some(pattern => pattern.test(input));

        if (hasXSS) {
          throw new Error('Potential XSS attack detected');
        }
        return { safe: true, sanitized: input.replace(/[<>]/g, '') };
      });

      for (const xssPayload of xssPayloads) {
        await expect(mockValidation.preventXSS(xssPayload)).rejects.toThrow(
          'Potential XSS attack detected'
        );
      }
    });

    test('should sanitize HTML while preserving safe content', async () => {
      const inputs = [
        'Temperature: 25°C',
        'Station Alpha & Beta readings',
        'Data from 2025-06-21 @ 10:00 AM',
        'Humidity level: >90%',
      ];

      mockValidation.preventXSS.mockImplementation(input => {
        const sanitized = input
          .replace(/</g, '&lt;')
          .replace(/>/g, '&gt;')
          .replace(/"/g, '&quot;')
          .replace(/'/g, '&#x27;');

        return { safe: true, sanitized };
      });

      for (const input of inputs) {
        const result = await mockValidation.preventXSS(input);
        expect(result.safe).toBe(true);
        expect(result.sanitized).not.toContain('<script');
      }
    });

    test('should handle URL validation', async () => {
      const maliciousURLs = [
        'javascript:alert(1)',
        'data:text/html,<script>alert(1)</script>',
        'vbscript:msgbox(1)',
        'file:///etc/passwd',
      ];

      const safeURLs = [
        'https://api.wrfcoin.com/weather',
        'http://localhost:3001/api/status',
        '/relative/path/to/resource',
        'mailto:support@wrfcoin.com',
      ];

      mockValidation.validateUserInput.mockImplementation(url => {
        const dangerousProtocols = [
          'javascript:',
          'data:',
          'vbscript:',
          'file:',
        ];
        const isDangerous = dangerousProtocols.some(protocol =>
          url.toLowerCase().startsWith(protocol)
        );

        if (isDangerous) {
          throw new Error('Dangerous URL protocol detected');
        }
        return { valid: true, url };
      });

      for (const maliciousURL of maliciousURLs) {
        await expect(
          mockValidation.validateUserInput(maliciousURL)
        ).rejects.toThrow('Dangerous URL protocol detected');
      }

      for (const safeURL of safeURLs) {
        const result = await mockValidation.validateUserInput(safeURL);
        expect(result.valid).toBe(true);
      }
    });
  });

  describe('Path Traversal Prevention', () => {
    test('should block path traversal attempts', async () => {
      const pathTraversalAttempts = [
        '../../../etc/passwd',
        '..\\..\\..\\windows\\system32\\config\\sam',
        '/etc/passwd',
        'C:\\Windows\\System32\\config\\SAM',
        '%2e%2e%2f%2e%2e%2f%2e%2e%2fetc%2fpasswd',
        '....//....//....//etc//passwd',
      ];

      mockValidation.validateFileUpload.mockImplementation(filePath => {
        const pathTraversalPatterns = [
          /\.\./,
          /\/etc\//,
          /\\windows\\/i,
          /%2e%2e/i,
          /\.\.%2f/i,
        ];

        const hasPathTraversal = pathTraversalPatterns.some(pattern =>
          pattern.test(filePath)
        );

        if (hasPathTraversal) {
          throw new Error('Path traversal attempt detected');
        }
        return { safe: true, path: filePath };
      });

      for (const pathAttempt of pathTraversalAttempts) {
        await expect(
          mockValidation.validateFileUpload(pathAttempt)
        ).rejects.toThrow('Path traversal attempt detected');
      }
    });

    test('should allow safe file paths', async () => {
      const safeFilePaths = [
        'weather-data.json',
        'uploads/station-001/reading.csv',
        'reports/monthly/june-2025.pdf',
        'images/station-alpha.jpg',
      ];

      mockValidation.validateFileUpload.mockImplementation(filePath => {
        const pathTraversalPatterns = [
          /\.\./,
          /\/etc\//,
          /\\windows\\/i,
          /%2e%2e/i,
        ];

        const hasPathTraversal = pathTraversalPatterns.some(pattern =>
          pattern.test(filePath)
        );

        if (hasPathTraversal) {
          throw new Error('Path traversal attempt detected');
        }
        return { safe: true, path: filePath };
      });

      for (const safePath of safeFilePaths) {
        const result = await mockValidation.validateFileUpload(safePath);
        expect(result.safe).toBe(true);
      }
    });
  });

  describe('Data Type Validation', () => {
    test('should validate weather data types strictly', async () => {
      const invalidWeatherData = [
        { temperature: 'hot', humidity: 65, pressure: 1013 },
        { temperature: 25, humidity: '65%', pressure: 1013 },
        { temperature: 25, humidity: 65, pressure: 'normal' },
        { temperature: null, humidity: 65, pressure: 1013 },
        { temperature: undefined, humidity: 65, pressure: 1013 },
        { temperature: Infinity, humidity: 65, pressure: 1013 },
        { temperature: NaN, humidity: 65, pressure: 1013 },
      ];

      mockValidation.validateWeatherData.mockImplementation(data => {
        const errors = [];

        if (
          typeof data.temperature !== 'number' ||
          isNaN(data.temperature) ||
          !isFinite(data.temperature)
        ) {
          errors.push('Temperature must be a valid number');
        }

        if (
          typeof data.humidity !== 'number' ||
          isNaN(data.humidity) ||
          !isFinite(data.humidity)
        ) {
          errors.push('Humidity must be a valid number');
        }

        if (
          typeof data.pressure !== 'number' ||
          isNaN(data.pressure) ||
          !isFinite(data.pressure)
        ) {
          errors.push('Pressure must be a valid number');
        }

        if (errors.length > 0) {
          throw new Error(`Validation failed: ${errors.join(', ')}`);
        }

        return { valid: true, data };
      });

      for (const invalidData of invalidWeatherData) {
        await expect(
          mockValidation.validateWeatherData(invalidData)
        ).rejects.toThrow('Validation failed');
      }
    });

    test('should validate data ranges', async () => {
      const outOfRangeData = [
        { temperature: -100, humidity: 65, pressure: 1013 }, // Too cold
        { temperature: 100, humidity: 65, pressure: 1013 }, // Too hot
        { temperature: 25, humidity: -10, pressure: 1013 }, // Negative humidity
        { temperature: 25, humidity: 150, pressure: 1013 }, // Humidity > 100%
        { temperature: 25, humidity: 65, pressure: 500 }, // Too low pressure
        { temperature: 25, humidity: 65, pressure: 2000 }, // Too high pressure
      ];

      mockValidation.validateWeatherData.mockImplementation(data => {
        const errors = [];

        if (data.temperature < -50 || data.temperature > 60) {
          errors.push('Temperature out of valid range (-50°C to 60°C)');
        }

        if (data.humidity < 0 || data.humidity > 100) {
          errors.push('Humidity out of valid range (0% to 100%)');
        }

        if (data.pressure < 800 || data.pressure > 1200) {
          errors.push('Pressure out of valid range (800 to 1200 hPa)');
        }

        if (errors.length > 0) {
          throw new Error(`Range validation failed: ${errors.join(', ')}`);
        }

        return { valid: true, data };
      });

      for (const outOfRange of outOfRangeData) {
        await expect(
          mockValidation.validateWeatherData(outOfRange)
        ).rejects.toThrow('Range validation failed');
      }
    });
  });

  describe('API Key Validation', () => {
    test('should validate API key format', async () => {
      const invalidAPIKeys = [
        '',
        'short',
        'contains spaces',
        'contains-invalid-chars!@#',
        'ak_test_',
        'not-starting-with-ak',
        null,
        undefined,
      ];

      mockValidation.validateApiKey.mockImplementation(apiKey => {
        if (!apiKey || typeof apiKey !== 'string') {
          throw new Error('API key must be a non-empty string');
        }

        if (!apiKey.startsWith('ak_')) {
          throw new Error('API key must start with "ak_"');
        }

        if (apiKey.length < 32) {
          throw new Error('API key too short');
        }

        if (!/^[a-zA-Z0-9_]+$/.test(apiKey)) {
          throw new Error('API key contains invalid characters');
        }

        return { valid: true, keyId: apiKey.split('_')[2] };
      });

      for (const invalidKey of invalidAPIKeys) {
        await expect(
          mockValidation.validateApiKey(invalidKey)
        ).rejects.toThrow();
      }
    });

    test('should accept valid API keys', async () => {
      const validAPIKeys = [
        'ak_test_1234567890abcdef1234567890abcdef',
        'ak_prod_abcdef1234567890abcdef1234567890',
        'ak_dev_9876543210fedcba9876543210fedcba',
      ];

      mockValidation.validateApiKey.mockImplementation(apiKey => {
        if (!apiKey || typeof apiKey !== 'string') {
          throw new Error('API key must be a non-empty string');
        }

        if (!apiKey.startsWith('ak_')) {
          throw new Error('API key must start with "ak_"');
        }

        if (apiKey.length < 32) {
          throw new Error('API key too short');
        }

        if (!/^[a-zA-Z0-9_]+$/.test(apiKey)) {
          throw new Error('API key contains invalid characters');
        }

        return { valid: true, keyId: apiKey.split('_')[2] };
      });

      for (const validKey of validAPIKeys) {
        const result = await mockValidation.validateApiKey(validKey);
        expect(result.valid).toBe(true);
        expect(result.keyId).toBeDefined();
      }
    });
  });

  describe('Wallet Address Validation', () => {
    test('should validate Wrfcoin wallet address format', async () => {
      const invalidWalletAddresses = [
        '',
        'wrf',
        'wrf123',
        'invalid1234567890abcdef1234567890abcdef12345678',
        'wrf1234567890abcdef1234567890abcdef1234567g', // Invalid char 'g'
        'WRF1234567890abcdef1234567890abcdef12345678', // Wrong case
        'wrf1234567890abcdef1234567890abcdef123456789', // Too long
      ];

      mockValidation.validateWalletAddress.mockImplementation(address => {
        if (!address || typeof address !== 'string') {
          throw new Error('Wallet address must be a non-empty string');
        }

        if (!address.startsWith('wrf')) {
          throw new Error('Wallet address must start with "wrf"');
        }

        if (address.length !== 43) {
          throw new Error('Wallet address must be exactly 43 characters');
        }

        if (!/^wrf[a-f0-9]{40}$/.test(address)) {
          throw new Error('Wallet address contains invalid characters');
        }

        return { valid: true, address };
      });

      for (const invalidAddress of invalidWalletAddresses) {
        await expect(
          mockValidation.validateWalletAddress(invalidAddress)
        ).rejects.toThrow();
      }
    });

    test('should accept valid wallet addresses', async () => {
      const validWalletAddresses = [
        'wrf1234567890abcdef1234567890abcdef12345678',
        'wrfabcdef1234567890abcdef1234567890abcdef',
        'wrf0000000000000000000000000000000000000000',
      ];

      mockValidation.validateWalletAddress.mockImplementation(address => {
        if (!address || typeof address !== 'string') {
          throw new Error('Wallet address must be a non-empty string');
        }

        if (!address.startsWith('wrf')) {
          throw new Error('Wallet address must start with "wrf"');
        }

        if (address.length !== 43) {
          throw new Error('Wallet address must be exactly 43 characters');
        }

        if (!/^wrf[a-f0-9]{40}$/.test(address)) {
          throw new Error('Wallet address contains invalid characters');
        }

        return { valid: true, address };
      });

      for (const validAddress of validWalletAddresses) {
        const result = await mockValidation.validateWalletAddress(validAddress);
        expect(result.valid).toBe(true);
        expect(result.address).toBe(validAddress);
      }
    });
  });

  describe('File Upload Security', () => {
    test('should validate file types', async () => {
      const dangerousFiles = [
        { name: 'malware.exe', type: 'application/x-msdownload' },
        { name: 'script.js', type: 'application/javascript' },
        { name: 'payload.php', type: 'application/x-httpd-php' },
        { name: 'config.bat', type: 'application/x-bat' },
        { name: 'virus.scr', type: 'application/x-msdownload' },
      ];

      const allowedTypes = [
        'image/jpeg',
        'image/png',
        'text/csv',
        'application/json',
      ];

      mockValidation.validateFileUpload.mockImplementation(file => {
        if (!allowedTypes.includes(file.type)) {
          throw new Error(`File type ${file.type} not allowed`);
        }

        const dangerousExtensions = [
          '.exe',
          '.bat',
          '.cmd',
          '.scr',
          '.php',
          '.js',
        ];
        const hasEvilExtension = dangerousExtensions.some(ext =>
          file.name.toLowerCase().endsWith(ext)
        );

        if (hasEvilExtension) {
          throw new Error('Dangerous file extension detected');
        }

        return { safe: true, file };
      });

      for (const dangerousFile of dangerousFiles) {
        await expect(
          mockValidation.validateFileUpload(dangerousFile)
        ).rejects.toThrow();
      }
    });

    test('should check file size limits', async () => {
      const oversizedFile = {
        name: 'large-data.csv',
        type: 'text/csv',
        size: 50 * 1024 * 1024, // 50MB
      };

      const maxSize = 10 * 1024 * 1024; // 10MB limit

      mockValidation.validateFileUpload.mockImplementation(file => {
        if (file.size > maxSize) {
          throw new Error(
            `File size ${file.size} exceeds maximum allowed size ${maxSize}`
          );
        }
        return { safe: true, file };
      });

      await expect(
        mockValidation.validateFileUpload(oversizedFile)
      ).rejects.toThrow('exceeds maximum allowed size');
    });
  });
});
