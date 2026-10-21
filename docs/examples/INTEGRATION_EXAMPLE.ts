/**
 * Gossip Security Integration Example
 * Task 100.14 - Complete integration example showing all security modules working together
 *
 * This file demonstrates how to integrate the secure gossip protocol into a
 * WRFCoin blockchain node with full security features enabled.
 */

import express from 'express';
import { WebSocketServer } from 'ws';
import { createServer } from 'http';
import { createGossipSecurityMiddleware, GossipSecurityMiddleware } from './backend/server/middleware/gossip-security-middleware.js';

/**
 * Complete Integration Example
 * Shows how to set up a secure gossip node with all security features
 */
async function setupSecureGossipNode() {
  // 1. Create Express application
  const app = express();
  app.use(express.json());

  // 2. Create HTTP server
  const httpServer = createServer(app);

  // 3. Create WebSocket server
  const wss = new WebSocketServer({ server: httpServer });

  // 4. Initialize gossip security middleware
  console.log('Initializing gossip security middleware...');
  const gossipSecurity = await createGossipSecurityMiddleware({
    enableEncryption: true,
    enableRateLimiting: true,
    enableAntiPoisoning: true,
    enableStateVerification: true,
    enableConsensusProtection: true,
    logLevel: 'info',
  });

  // 5. Apply HTTP middleware for gossip endpoints
  app.use('/api/gossip', gossipSecurity.httpMiddleware());

  // 6. HTTP endpoint for submitting gossip messages
  app.post('/api/gossip/message', async (req, res) => {
    try {
      const { messageId, topic, payload, priority, source } = req.body;

      console.log(`Received gossip message: ${messageId} on topic: ${topic}`);

      // Validate the message
      const validation = await gossipSecurity.validateGossipMessage(
        { messageId, topic, payload, priority },
        source || (req as any).security?.source || 'unknown'
      );

      if (!validation.allowed) {
        return res.status(400).json({
          success: false,
          error: 'Message rejected',
          reason: validation.reason,
          stage: validation.stage,
        });
      }

      // Message validated - process it
      console.log(`Message ${messageId} validated successfully`);

      res.json({
        success: true,
        messageId,
        timestamp: new Date().toISOString(),
      });
    } catch (error) {
      console.error('Error processing gossip message:', error);
      res.status(500).json({
        success: false,
        error: 'Internal server error',
      });
    }
  });

  // 7. HTTP endpoint for consensus messages
  app.post('/api/gossip/consensus', async (req, res) => {
    try {
      const consensusMessage = req.body;

      const validation = await gossipSecurity.validateConsensusMessage(consensusMessage);

      if (!validation.allowed) {
        return res.status(400).json({
          success: false,
          error: 'Consensus message rejected',
          reason: validation.reason,
        });
      }

      res.json({
        success: true,
        messageId: consensusMessage.messageId,
        accepted: true,
      });
    } catch (error) {
      console.error('Error processing consensus message:', error);
      res.status(500).json({
        success: false,
        error: 'Internal server error',
      });
    }
  });

  // 8. HTTP endpoint for state verification
  app.post('/api/gossip/verify-state', async (req, res) => {
    try {
      const { remoteStates } = req.body;

      const verification = await gossipSecurity.verifyNetworkState(
        new Map(Object.entries(remoteStates))
      );

      res.json({
        success: true,
        isValid: verification.isValid,
        consistency: verification.consistency,
        divergenceCount: verification.divergenceCount,
      });
    } catch (error) {
      console.error('Error verifying state:', error);
      res.status(500).json({
        success: false,
        error: 'Internal server error',
      });
    }
  });

  // 9. HTTP endpoint for security statistics
  app.get('/api/gossip/stats', (req, res) => {
    const stats = gossipSecurity.getStatistics();
    res.json({
      success: true,
      statistics: stats,
      timestamp: new Date().toISOString(),
    });
  });

  // 10. WebSocket connection handler with security
  wss.on('connection', async (ws, req) => {
    console.log('New WebSocket connection');

    // Apply security middleware
    const wsHandler = gossipSecurity.websocketMiddleware();
    wsHandler(ws, req);
  });

  // 11. Security event listeners
  gossipSecurity.on('log', (logData) => {
    if (logData.level === 'error' || logData.level === 'warn') {
      console.log(`[SECURITY ${logData.level.toUpperCase()}]`, logData.message, logData);
    }
  });

  // 12. Start server
  const PORT = process.env.PORT || 4002;
  httpServer.listen(PORT, () => {
    console.log(`Secure gossip node listening on port ${PORT}`);
    console.log(`HTTP API: http://localhost:${PORT}/api/gossip`);
    console.log(`WebSocket: ws://localhost:${PORT}`);
    console.log('Security features enabled:');
    console.log('  ✓ Encryption (topic-based)');
    console.log('  ✓ Rate limiting (adaptive)');
    console.log('  ✓ Anti-poisoning (statistical)');
    console.log('  ✓ State verification (Merkle)');
    console.log('  ✓ Consensus protection (Byzantine)');
  });

  // 13. Graceful shutdown
  process.on('SIGTERM', () => {
    console.log('SIGTERM received, shutting down gracefully...');
    httpServer.close(() => {
      gossipSecurity.shutdown();
      console.log('Server shut down');
      process.exit(0);
    });
  });

  return { app, httpServer, wss, gossipSecurity };
}

/**
 * Example: Sending a secure gossip message
 */
async function sendSecureGossipMessage(
  gossipSecurity: GossipSecurityMiddleware,
  topic: string,
  payload: any,
  source: string
) {
  const services = gossipSecurity.getServices();

  // Create secure message with all protections
  const message = await services.secureGossip.createSecureGossipMessage(
    topic,
    Buffer.from(JSON.stringify(payload)),
    source,
    {
      priority: 'high',
      requiresVerification: true,
      minValidators: 3,
      crossVerificationCount: 0,
    }
  );

  console.log('Created secure gossip message:', {
    messageId: message.messageId,
    topic: message.topic,
    encrypted: message.encryptionMetadata?.encrypted,
    hopCount: message.hopCount,
  });

  return message;
}

/**
 * Example: Performing state verification
 */
async function performStateVerification(gossipSecurity: GossipSecurityMiddleware) {
  const services = gossipSecurity.getServices();

  // Update local state
  services.stateVerifier.updateState('block_height', 12345);
  services.stateVerifier.updateState('validator_set', ['val1', 'val2', 'val3']);

  // Create snapshot
  const snapshot = services.stateVerifier.createSnapshot();
  console.log('State snapshot created:', snapshot.snapshotId);

  // Get current state
  const currentState = services.stateVerifier.getCurrentState();
  console.log('Current state version:', currentState?.version);

  return snapshot;
}

/**
 * Example: Detecting and handling poisoned messages
 */
async function detectPoisoning(
  gossipSecurity: GossipSecurityMiddleware,
  messageId: string,
  topic: string,
  payload: any,
  source: string
) {
  const services = gossipSecurity.getServices();

  const detection = await services.antiPoisoning.detectPoisoning(
    messageId,
    topic,
    payload,
    source
  );

  if (detection.isPoisoned) {
    console.log('⚠️  Poisoned message detected!');
    console.log('  Confidence:', detection.confidence);
    console.log('  Action:', detection.recommendedAction);
    console.log('  Reasons:', detection.reasons);
  } else {
    console.log('✓ Message appears legitimate');
  }

  return detection;
}

/**
 * Example: Monitoring rate limits
 */
async function monitorRateLimits(gossipSecurity: GossipSecurityMiddleware) {
  const services = gossipSecurity.getServices();

  // Check rate limit for a source
  const result = await services.rateLimiter.checkRateLimit(
    'test_msg_1',
    'peer1',
    'test_topic',
    'normal'
  );

  console.log('Rate limit check:', {
    allowed: result.allowed,
    tokensRemaining: result.tokensRemaining,
    currentRate: result.currentRate,
    limit: result.limit,
  });

  // Get rate limiter statistics
  const stats = services.rateLimiter.getStatistics();
  console.log('Rate limiter stats:', stats);

  return result;
}

/**
 * Example: Validating consensus messages
 */
async function validateConsensusMessage(
  gossipSecurity: GossipSecurityMiddleware,
  consensusMessage: any
) {
  const services = gossipSecurity.getServices();

  const validation = await services.consensusProtection.validateConsensusMessage(
    consensusMessage
  );

  if (validation.isValid) {
    console.log('✓ Consensus message validated');
  } else {
    console.log('✗ Consensus message rejected:', validation.reasons);
  }

  // Check for split-brain
  const splitBrain = services.consensusProtection.detectSplitBrain(
    consensusMessage.height
  );

  if (splitBrain.detected) {
    console.log('⚠️  Split-brain detected!');
    console.log('  Partition 1:', splitBrain.partition1Size);
    console.log('  Partition 2:', splitBrain.partition2Size);
    console.log('  Confidence:', splitBrain.confidence);
  }

  return validation;
}

/**
 * Main execution
 */
if (require.main === module) {
  setupSecureGossipNode().catch(error => {
    console.error('Failed to start secure gossip node:', error);
    process.exit(1);
  });
}

// Export for use in other modules
export {
  setupSecureGossipNode,
  sendSecureGossipMessage,
  performStateVerification,
  detectPoisoning,
  monitorRateLimits,
  validateConsensusMessage,
};
