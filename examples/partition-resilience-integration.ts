/**
 * Network Partition Resilience - Integration Example
 * Demonstrates how to integrate all partition resilience components
 *
 * This example shows a complete setup of the partition resilience system
 * with all components working together.
 */

import express from 'express';
import { NetworkPartitionDetector, NetworkNode } from '../backend/shared/network-partition-detector';
import { PartitionTolerantConsensus, ConsensusNode } from '../backend/shared/partition-tolerant-consensus';
import { SplitBrainPrevention } from '../backend/shared/split-brain-prevention';
import { StateReconciliation } from '../backend/shared/state-reconciliation';
import { CheckpointManager } from '../backend/shared/checkpoint-manager';
import { PartitionRecovery } from '../backend/shared/partition-recovery';
import { NetworkTopologyMonitor, NetworkNode as TopologyNode } from '../backend/shared/network-topology-monitor';
import {
  createPartitionResilienceMiddleware,
  createPartitionStatusHandler,
  createHealthCheckMiddleware,
} from '../backend/server/middleware/partition-resilience-middleware';

/**
 * Initialize all partition resilience components
 */
async function initializePartitionResilience() {
  console.log('🚀 Initializing Network Partition Resilience System');

  // 1. Create core components
  const partitionDetector = new NetworkPartitionDetector({
    heartbeatInterval: 5000,
    failureThreshold: 3,
    partitionThreshold: 30,
    validatorWeightFactor: 3.0,
  });

  const consensus = new PartitionTolerantConsensus('node-1', {
    consensusType: 'raft',
    electionTimeout: 5000,
    heartbeatInterval: 1000,
    haltOnPartition: true,
  });

  const splitBrainPrevention = new SplitBrainPrevention('node-1', {
    minimumQuorumSize: 4,
    minimumQuorumPercentage: 67,
    minimumStakePercentage: 67,
    haltOnMinorityPartition: true,
  });

  const stateReconciliation = new StateReconciliation({
    conflictResolutionStrategy: 'majority',
    enableDoubleSpendDetection: true,
  });

  const checkpointManager = new CheckpointManager({
    checkpointInterval: 100,
    minValidatorSignatures: 4,
    minStakePercentage: 67,
  });

  const topologyMonitor = new NetworkTopologyMonitor({
    snapshotInterval: 10000,
    predictionInterval: 30000,
    enablePrediction: true,
  });

  const partitionRecovery = new PartitionRecovery(
    partitionDetector,
    stateReconciliation,
    checkpointManager,
    splitBrainPrevention,
    consensus,
    {
      enableAutomaticRecovery: true,
      requireCheckpointValidation: true,
    }
  );

  // 2. Register validators
  console.log('📝 Registering validators');
  const validators = [
    { id: 'validator-1', stake: BigInt(10000) },
    { id: 'validator-2', stake: BigInt(8000) },
    { id: 'validator-3', stake: BigInt(7000) },
    { id: 'validator-4', stake: BigInt(6000) },
    { id: 'validator-5', stake: BigInt(5000) },
  ];

  for (const validator of validators) {
    // Register in checkpoint manager
    checkpointManager.registerValidator(
      validator.id,
      `pubkey-${validator.id}`,
      validator.stake
    );

    // Register in split-brain prevention
    splitBrainPrevention.registerValidator(validator.id, validator.stake);

    // Register in consensus
    const consensusNode: ConsensusNode = {
      nodeId: validator.id,
      isValidator: true,
      stake: validator.stake,
      term: 0,
      votedFor: null,
      lastHeartbeat: new Date(),
      role: 'follower',
    };
    consensus.registerNode(consensusNode);
  }

  // 3. Register network nodes
  console.log('🌐 Registering network nodes');
  const networkNodes = [
    { id: 'node-1', region: 'us-west', asn: 'AS1234', isValidator: true },
    { id: 'node-2', region: 'us-west', asn: 'AS1234', isValidator: true },
    { id: 'node-3', region: 'us-east', asn: 'AS5678', isValidator: true },
    { id: 'node-4', region: 'eu-west', asn: 'AS9012', isValidator: true },
    { id: 'node-5', region: 'ap-south', asn: 'AS3456', isValidator: true },
  ];

  for (const node of networkNodes) {
    // Register in partition detector
    const partitionNode: NetworkNode = {
      nodeId: node.id,
      address: `192.168.1.${networkNodes.indexOf(node)}`,
      port: 3000 + networkNodes.indexOf(node),
      region: node.region,
      asn: node.asn,
      isValidator: node.isValidator,
      stake: node.isValidator ? BigInt(5000 + networkNodes.indexOf(node) * 1000) : undefined,
      lastSeen: new Date(),
      consecutiveFailures: 0,
    };
    partitionDetector.registerNode(partitionNode);

    // Register in topology monitor
    const topologyNode: TopologyNode = {
      nodeId: node.id,
      address: `192.168.1.${networkNodes.indexOf(node)}`,
      port: 3000 + networkNodes.indexOf(node),
      region: node.region,
      asn: node.asn,
      isValidator: node.isValidator,
      connections: [],
      latency: new Map(),
      lastSeen: new Date(),
      status: 'online',
    };
    topologyMonitor.registerNode(topologyNode);
  }

  // 4. Setup event listeners
  console.log('🎧 Setting up event listeners');

  partitionDetector.on('partition_detected', (event) => {
    console.error('🚨 PARTITION DETECTED:', {
      partitionId: event.partitionId,
      severity: event.severity,
      affectedNodes: event.affectedNodes.length,
      affectedValidators: event.affectedValidators.length,
    });

    // Notify other components
    consensus.handlePartitionEvent(event);
    splitBrainPrevention.handlePartitionEvent(event);
  });

  partitionDetector.on('partition_healed', (event) => {
    console.log('✅ PARTITION HEALED:', event);
    // Recovery will be handled automatically by PartitionRecovery
  });

  consensus.on('consensus_halted', ({ reason }) => {
    console.warn('⚠️ CONSENSUS HALTED:', reason);
  });

  consensus.on('consensus_resumed', () => {
    console.log('✅ CONSENSUS RESUMED');
  });

  splitBrainPrevention.on('split_brain_detected', (detection) => {
    console.error('🚨 SPLIT-BRAIN DETECTED:', detection);
  });

  topologyMonitor.on('partition_predicted', (prediction) => {
    console.warn('⚠️ PARTITION PREDICTED:', {
      probability: (prediction.probability * 100).toFixed(1) + '%',
      severity: prediction.severity,
      estimatedTime: prediction.estimatedTimeToPartition,
    });
  });

  partitionRecovery.on('recovery_started', (recovery) => {
    console.log('🔄 RECOVERY STARTED:', recovery.recoveryId);
  });

  partitionRecovery.on('recovery_completed', (recovery) => {
    console.log('✅ RECOVERY COMPLETED:', {
      recoveryId: recovery.recoveryId,
      duration: recovery.completedAt
        ? recovery.completedAt.getTime() - recovery.startedAt.getTime()
        : 0,
      success: recovery.success,
    });
  });

  // 5. Start all services
  console.log('▶️ Starting services');
  await partitionDetector.start();
  await consensus.start();
  await splitBrainPrevention.start();
  await topologyMonitor.start();

  console.log('✅ All services started successfully');

  return {
    partitionDetector,
    consensus,
    splitBrainPrevention,
    stateReconciliation,
    checkpointManager,
    partitionRecovery,
    topologyMonitor,
  };
}

/**
 * Setup Express server with partition resilience middleware
 */
async function setupExpressServer(components: any) {
  const app = express();
  app.use(express.json());

  console.log('🌐 Setting up Express server');

  // Apply partition resilience middleware globally
  app.use(
    createPartitionResilienceMiddleware(
      components.partitionDetector,
      components.splitBrainPrevention,
      components.consensus,
      components.topologyMonitor,
      {
        enablePartitionCheck: true,
        allowReadsDuringPartition: true,
        allowWritesDuringPartition: false,
        requireConsensusForWrites: true,
      }
    )
  );

  // Health check endpoint
  app.get(
    '/health',
    createHealthCheckMiddleware(
      components.partitionDetector,
      components.topologyMonitor
    )
  );

  // Partition status endpoint
  app.get(
    '/api/partition/status',
    createPartitionStatusHandler(
      components.partitionDetector,
      components.splitBrainPrevention,
      components.consensus,
      components.topologyMonitor
    )
  );

  // Example read endpoint (allowed during partition)
  app.get('/api/data/:id', (req, res) => {
    res.json({
      id: req.params.id,
      data: 'Sample data',
      timestamp: new Date().toISOString(),
    });
  });

  // Example write endpoint (blocked during partition)
  app.post('/api/data', (req, res) => {
    res.json({
      success: true,
      message: 'Data written successfully',
      timestamp: new Date().toISOString(),
    });
  });

  // Example checkpoint creation endpoint
  app.post('/api/checkpoint', async (req, res) => {
    try {
      const accounts = new Map();
      accounts.set('alice', {
        address: 'alice',
        balance: BigInt(1000),
        nonce: 1,
        stateHash: 'hash1',
      });

      const checkpoint = await components.checkpointManager.createCheckpoint(
        req.body.blockHeight || 100,
        req.body.blockHash || 'test-block-hash',
        req.body.stateRoot || 'test-state-root',
        [],
        accounts
      );

      res.json({
        success: true,
        checkpointId: checkpoint.checkpointId,
        blockHeight: checkpoint.blockHeight,
      });
    } catch (error) {
      res.status(500).json({
        error: 'Failed to create checkpoint',
        message: String(error),
      });
    }
  });

  const PORT = process.env.PORT || 4001;
  app.listen(PORT, () => {
    console.log(`✅ Server running on port ${PORT}`);
  });

  return app;
}

/**
 * Simulate network partition scenario
 */
async function simulatePartitionScenario(components: any) {
  console.log('\n🎭 Simulating network partition scenario...\n');

  await new Promise((resolve) => setTimeout(resolve, 2000));

  // Simulate nodes becoming unreachable
  console.log('1️⃣ Simulating node failures (30% of network)');
  for (let i = 0; i < 3; i++) {
    components.partitionDetector.recordProbe({
      targetNodeId: `node-${i + 3}`,
      probedAt: new Date(),
      success: false,
      error: 'Connection timeout',
    });
  }

  await new Promise((resolve) => setTimeout(resolve, 5000));

  // Partition should be detected
  const status = components.partitionDetector.getPartitionStatus();
  if (status.hasActivePartition) {
    console.log('✅ Partition detected successfully');
  }

  await new Promise((resolve) => setTimeout(resolve, 3000));

  // Heal the partition
  console.log('2️⃣ Healing partition (nodes coming back online)');
  for (let i = 0; i < 3; i++) {
    components.partitionDetector.recordProbe({
      targetNodeId: `node-${i + 3}`,
      probedAt: new Date(),
      success: true,
      latency: 50,
    });
  }

  await new Promise((resolve) => setTimeout(resolve, 5000));

  console.log('✅ Simulation complete\n');
}

/**
 * Main entry point
 */
async function main() {
  try {
    console.log('═══════════════════════════════════════════════════');
    console.log('  Network Partition Resilience - Integration Demo');
    console.log('═══════════════════════════════════════════════════\n');

    // Initialize all components
    const components = await initializePartitionResilience();

    // Setup Express server
    await setupExpressServer(components);

    // Run simulation
    await simulatePartitionScenario(components);

    console.log('═══════════════════════════════════════════════════');
    console.log('  System is now running and monitoring for partitions');
    console.log('  Press Ctrl+C to exit');
    console.log('═══════════════════════════════════════════════════\n');

    // Keep process running
    process.on('SIGINT', () => {
      console.log('\n\n🛑 Shutting down gracefully...');
      components.partitionDetector.stop();
      components.consensus.stop();
      components.splitBrainPrevention.stop();
      components.topologyMonitor.stop();
      process.exit(0);
    });
  } catch (error) {
    console.error('❌ Error:', error);
    process.exit(1);
  }
}

// Run if executed directly
if (require.main === module) {
  main();
}

export { initializePartitionResilience, setupExpressServer, simulatePartitionScenario };
