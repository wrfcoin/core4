# Network Simulation Testing Guide

This guide demonstrates how to use the Wrfcoin Network Simulation utilities to test blockchain
behavior under various network conditions, including partitions, Byzantine faults, and chain splits.

## Table of Contents

1. [Basic Network Simulation](#basic-network-simulation)
2. [Network Topology Testing](#network-topology-testing)
3. [Byzantine Fault Tolerance](#byzantine-fault-tolerance)
4. [Chain Split Testing](#chain-split-testing)
5. [Advanced Network Scenarios](#advanced-network-scenarios)

## Basic Network Simulation

Start by creating a basic network simulation:

```javascript
const { NetworkSimulator, SimulatedNode } = require('../../utils/testing');

// Create a network simulator
const networkSim = new NetworkSimulator({
  consensusProtocol: 'pow', // Proof of Work
  blockTime: 10000, // ms
});

// Add nodes with different roles
networkSim.addNode({
  id: 'validator-1',
  type: 'validator',
  capabilities: { mining: true, validation: true },
});

networkSim.addNode({
  id: 'miner-1',
  type: 'miner',
  capabilities: { mining: true },
});

networkSim.addNode({
  id: 'miner-2',
  type: 'miner',
  capabilities: { mining: true },
});

// Add full nodes (no mining)
for (let i = 1; i <= 5; i++) {
  networkSim.addNode({
    id: `full-node-${i}`,
    type: 'full',
  });
}

// Add light clients
for (let i = 1; i <= 3; i++) {
  networkSim.addNode({
    id: `light-client-${i}`,
    type: 'light',
  });
}

// Create a network topology
networkSim.createTopology('random');

// Start simulation
networkSim.startMining();

// Wait for some blocks to be created
setTimeout(() => {
  // Get network statistics
  const stats = networkSim.getNetworkStats();
  console.log('Network statistics:', stats);

  // Get consensus status
  const consensus = networkSim.getConsensusStatus();
  console.log('Consensus status:', consensus);

  // Check blockchain state on individual nodes
  const validator = networkSim.getNode('validator-1');
  console.log('Validator blockchain height:', validator.blockchain.blocks.length);

  const miner = networkSim.getNode('miner-1');
  console.log('Miner blockchain height:', miner.blockchain.blocks.length);

  const fullNode = networkSim.getNode('full-node-1');
  console.log('Full node blockchain height:', fullNode.blockchain.blocks.length);

  // Stop the simulation
  networkSim.stop();
}, 60000); // Run for 60 seconds
```

## Network Topology Testing

Test different network topologies and their impact on block propagation:

```javascript
const { NetworkSimulator } = require('../../utils/testing');

// Function to run tests on different topologies
async function testTopology(topology) {
  console.log(`\nTesting ${topology} topology:`);

  // Create a simulator
  const sim = new NetworkSimulator({
    consensusProtocol: 'pow',
    blockTime: 5000, // Faster block time for testing
  });

  // Add nodes (10 miners, 20 full nodes)
  for (let i = 1; i <= 10; i++) {
    sim.addNode({
      id: `miner-${i}`,
      type: 'miner',
      capabilities: { mining: true },
    });
  }

  for (let i = 1; i <= 20; i++) {
    sim.addNode({
      id: `node-${i}`,
      type: 'full',
    });
  }

  // Create the specified topology
  sim.createTopology(topology);

  // Start mining
  sim.startMining();

  // Run for 30 seconds
  await new Promise(resolve => setTimeout(resolve, 30000));

  // Get consensus status
  const consensus = sim.getConsensusStatus();
  const stats = sim.getNetworkStats();

  console.log(`Consensus percentage: ${consensus.consensusPercentage.toFixed(2)}%`);
  console.log(`Average block height: ${consensus.averageBlockHeight.toFixed(2)}`);
  console.log(`Forks: ${consensus.forks.length}`);
  console.log(`Average connections per node: ${stats.avgConnections.toFixed(2)}`);
  console.log(`Average latency: ${stats.avgLatency.toFixed(2)} ms`);

  // Stop simulation
  sim.stop();

  return {
    topology,
    consensusPercentage: consensus.consensusPercentage,
    forks: consensus.forks.length,
    avgConnections: stats.avgConnections,
    avgLatency: stats.avgLatency,
  };
}

// Test different topologies
async function runTopologyTests() {
  const results = [];

  // Test each topology
  for (const topology of ['ring', 'star', 'fully_connected', 'random']) {
    results.push(await testTopology(topology));
  }

  // Compare results
  console.log('\nTopology Comparison:');
  console.table(results);
}

runTopologyTests();
```

## Byzantine Fault Tolerance

Test how the network handles Byzantine (malicious or faulty) nodes:

```javascript
const { NetworkSimulator, ByzantineSimulator } = require('../../utils/testing');

// Create network and Byzantine simulators
const networkSim = new NetworkSimulator({
  consensusProtocol: 'pos', // Proof of Stake for BFT testing
  blockTime: 5000,
});

// Create a mix of validator and non-validator nodes
for (let i = 1; i <= 21; i++) {
  // 21 nodes for 2/3 majority
  networkSim.addNode({
    id: `validator-${i}`,
    type: 'validator',
    capabilities: { validation: true, mining: true },
  });
}

for (let i = 1; i <= 10; i++) {
  networkSim.addNode({
    id: `node-${i}`,
    type: 'full',
  });
}

// Create a network topology
networkSim.createTopology('fully_connected');

// Create a Byzantine simulator
const byzantineSim = new ByzantineSimulator(networkSim);

// Test tolerance at different Byzantine ratios
async function testByzantineTolerance() {
  const results = [];

  // Test with different percentages of Byzantine nodes
  for (const ratio of [0.1, 0.2, 0.3, 0.4]) {
    console.log(`\nTesting with ${ratio * 100}% Byzantine nodes:`);

    // Run the Byzantine test
    const testResult = await byzantineSim.runByzantineTest(ratio, 'random');
    results.push({
      byzantineRatio: ratio,
      consensusPercentage: testResult.consensusPercentage,
      toleranceHeld: testResult.toleranceHeld,
      forks: testResult.forks,
    });

    console.log(`Byzantine nodes: ${testResult.byzantineNodes}`);
    console.log(`Consensus percentage: ${testResult.consensusPercentage.toFixed(2)}%`);
    console.log(`Tolerance held: ${testResult.toleranceHeld}`);
    console.log(`Forks: ${testResult.forks}`);
  }

  // Compare results
  console.log('\nByzantine Tolerance Results:');
  console.table(results);

  // Determine maximum Byzantine tolerance
  const maxTolerance = results.filter(r => r.toleranceHeld).pop();
  console.log(
    `Maximum Byzantine tolerance: ${maxTolerance ? maxTolerance.byzantineRatio * 100 : 0}%`
  );
}

testByzantineTolerance();
```

## Chain Split Testing

Test how the network handles and resolves chain splits:

```javascript
const { NetworkSimulator, ChainSplitSimulator } = require('../../utils/testing');

// Create simulators
const networkSim = new NetworkSimulator({
  consensusProtocol: 'pow',
  blockTime: 2000, // Faster for testing
});

// Add nodes (miners in different geographic regions)
// Region A miners
for (let i = 1; i <= 5; i++) {
  networkSim.addNode({
    id: `region-a-miner-${i}`,
    type: 'miner',
    capabilities: { mining: true },
  });
}

// Region B miners
for (let i = 1; i <= 5; i++) {
  networkSim.addNode({
    id: `region-b-miner-${i}`,
    type: 'miner',
    capabilities: { mining: true },
  });
}

// Full nodes in both regions
for (let i = 1; i <= 10; i++) {
  networkSim.addNode({
    id: `region-a-node-${i}`,
    type: 'full',
  });

  networkSim.addNode({
    id: `region-b-node-${i}`,
    type: 'full',
  });
}

// Create topology
networkSim.createTopology('random');

// Create chain split simulator
const chainSplitSim = new ChainSplitSimulator(networkSim);

// Run a chain split test
async function testChainSplit() {
  // Start mining
  networkSim.startMining();

  // Let the blockchain grow a bit
  console.log('Building initial blockchain...');
  await new Promise(resolve => setTimeout(resolve, 20000));

  // Check initial state
  const initialConsensus = networkSim.getConsensusStatus();
  console.log(
    'Initial consensus percentage:',
    initialConsensus.consensusPercentage.toFixed(2) + '%'
  );
  console.log('Initial median block height:', initialConsensus.medianBlockHeight);

  // Create network partition (chain split)
  console.log('\nCreating network partition...');
  const splitResult = chainSplitSim.createChainSplit(2, {
    distributeMiningNodes: true, // Ensure miners on both sides of split
  });

  console.log(`Created ${splitResult.partitionCount} partitions:`);
  splitResult.partitions.forEach((p, i) => {
    console.log(`Partition ${i + 1}: ${p.nodes.length} nodes, ${p.miningNodes} miners`);
  });

  // Let the chains diverge
  console.log('\nLetting chains diverge...');
  await new Promise(resolve => setTimeout(resolve, 30000));

  // Run a formal chain split test
  const splitTestResult = await chainSplitSim.runChainSplitTest(30000, {
    healAfterTest: false, // Don't heal yet
  });

  console.log('\nChain split state:');
  splitTestResult.partitionStates.forEach((state, i) => {
    console.log(`Partition ${i + 1}:`);
    console.log(`  Block height range: ${state.minHeight}-${state.maxHeight}`);
    console.log(`  Divergence occurred at block: ${state.divergenceHeight || 'N/A'}`);
    console.log(`  Internal consensus: ${state.consensusPercentage.toFixed(2)}%`);
  });

  // Check if chains have diverged
  if (splitTestResult.diverged) {
    console.log('\nCHAIN SPLIT DETECTED - Blockchains have diverged');
  } else {
    console.log('\nNo chain divergence detected');
  }

  // Heal the network
  console.log('\nHealing the network partition...');
  const healingResult = await chainSplitSim.healChainSplit();

  console.log('Healing result:');
  console.log(`  Forks resolved: ${healingResult.forksResolved}`);
  console.log(`  Consensus before healing: ${healingResult.consensusBefore.toFixed(2)}%`);
  console.log(`  Consensus after healing: ${healingResult.consensusAfter.toFixed(2)}%`);
  console.log(`  Remaining forks: ${healingResult.remainingForks.length}`);

  // Let the network stabilize
  console.log('\nLetting network stabilize...');
  await new Promise(resolve => setTimeout(resolve, 20000));

  // Check final consensus
  const finalConsensus = networkSim.getConsensusStatus();
  console.log('\nFinal network state:');
  console.log(`  Consensus percentage: ${finalConsensus.consensusPercentage.toFixed(2)}%`);
  console.log(`  Median block height: ${finalConsensus.medianBlockHeight}`);
  console.log(`  Remaining forks: ${finalConsensus.forks.length}`);

  // Stop the simulation
  networkSim.stop();
}

testChainSplit();
```

## Advanced Network Scenarios

### Testing Network Degradation

```javascript
const { NetworkSimulator } = require('../../utils/testing');

// Create simulator
const networkSim = new NetworkSimulator({
  consensusProtocol: 'pow',
  blockTime: 5000,
});

// Add nodes
for (let i = 1; i <= 5; i++) {
  networkSim.addNode({
    id: `miner-${i}`,
    type: 'miner',
    capabilities: { mining: true },
  });
}

for (let i = 1; i <= 15; i++) {
  networkSim.addNode({
    id: `node-${i}`,
    type: 'full',
  });
}

// Create topology
networkSim.createTopology('random');

// Start mining
networkSim.startMining();

// Run degradation test
async function testNetworkDegradation() {
  // Let the network stabilize
  console.log('Building initial blockchain...');
  await new Promise(resolve => setTimeout(resolve, 15000));

  // Measure initial performance
  const initialConsensus = networkSim.getConsensusStatus();
  const initialStats = networkSim.getNetworkStats();

  console.log('Initial network performance:');
  console.log(`  Consensus: ${initialConsensus.consensusPercentage.toFixed(2)}%`);
  console.log(`  Average block height: ${initialConsensus.averageBlockHeight.toFixed(2)}`);
  console.log(`  Average latency: ${initialStats.avgLatency.toFixed(2)} ms`);

  // Degrade network for a subset of nodes
  console.log('\nDegrading network quality for 30% of nodes...');
  const nodeIds = Array.from(networkSim.nodes.keys());
  const nodesToDegrade = Math.ceil(nodeIds.length * 0.3);

  for (let i = 0; i < nodesToDegrade; i++) {
    networkSim.degradeNodeNetwork(nodeIds[i], {
      latencyFactor: 5, // 5x higher latency
      packetLossFactor: 0.1, // +10% packet loss
      bandwidthFactor: 5, // 5x lower bandwidth
    });
  }

  // Let the network run with degraded conditions
  console.log('Running with degraded network conditions...');
  await new Promise(resolve => setTimeout(resolve, 30000));

  // Measure degraded performance
  const degradedConsensus = networkSim.getConsensusStatus();
  const degradedStats = networkSim.getNetworkStats();

  console.log('\nDegraded network performance:');
  console.log(`  Consensus: ${degradedConsensus.consensusPercentage.toFixed(2)}%`);
  console.log(`  Average block height: ${degradedConsensus.averageBlockHeight.toFixed(2)}`);
  console.log(`  Average latency: ${degradedStats.avgLatency.toFixed(2)} ms`);
  console.log(
    `  Impact on consensus: ${initialConsensus.consensusPercentage - degradedConsensus.consensusPercentage}%`
  );

  // Reset network quality
  console.log('\nResetting network conditions...');
  nodeIds.forEach(id => {
    const node = networkSim.getNode(id);
    node.resetNetworkQuality();
  });

  // Let the network recover
  console.log('Letting network recover...');
  await new Promise(resolve => setTimeout(resolve, 20000));

  // Measure recovered performance
  const recoveredConsensus = networkSim.getConsensusStatus();

  console.log('\nRecovered network performance:');
  console.log(`  Consensus: ${recoveredConsensus.consensusPercentage.toFixed(2)}%`);
  console.log(`  Average block height: ${recoveredConsensus.averageBlockHeight.toFixed(2)}`);

  // Stop the simulation
  networkSim.stop();
}

testNetworkDegradation();
```

### Testing Sybil Attack Resistance

```javascript
const { NetworkSimulator } = require('../../utils/testing');

// Create simulator
const networkSim = new NetworkSimulator({
  consensusProtocol: 'pow',
  blockTime: 5000,
});

// Add legitimate nodes
for (let i = 1; i <= 5; i++) {
  networkSim.addNode({
    id: `validator-${i}`,
    type: 'validator',
    capabilities: { validation: true, mining: true },
  });
}

for (let i = 1; i <= 15; i++) {
  networkSim.addNode({
    id: `node-${i}`,
    type: 'full',
  });
}

// Create topology
networkSim.createTopology('random');

// Start mining
networkSim.startMining();

// Run Sybil attack test
async function testSybilAttack() {
  // Let the network stabilize
  console.log('Building initial blockchain...');
  await new Promise(resolve => setTimeout(resolve, 15000));

  // Measure initial network state
  const initialStats = networkSim.getNetworkStats();
  const initialConsensus = networkSim.getConsensusStatus();

  console.log('Initial network state:');
  console.log(`  Nodes: ${initialStats.nodeCount}`);
  console.log(`  Consensus: ${initialConsensus.consensusPercentage.toFixed(2)}%`);

  // Launch Sybil attack
  console.log('\nLaunching Sybil attack...');
  const sybilCount = 30; // Large number of Sybil nodes
  const attackResults = networkSim.runSybilAttack(sybilCount, {
    honeypotRatio: 0.8, // Target 80% of the most connected nodes
  });

  console.log('Sybil attack results:');
  console.log(`  Sybil nodes created: ${attackResults.sybilNodes.length}`);
  console.log(`  Connections established: ${attackResults.connectionsEstablished}`);
  console.log(`  Network influence: ${(attackResults.networkInfluence * 100).toFixed(2)}%`);

  // Let the attack run
  console.log('\nRunning with Sybil nodes...');
  await new Promise(resolve => setTimeout(resolve, 30000));

  // Measure network state under attack
  const attackStats = networkSim.getNetworkStats();
  const attackConsensus = networkSim.getConsensusStatus();

  console.log('\nNetwork state under Sybil attack:');
  console.log(`  Total nodes: ${attackStats.nodeCount}`);
  console.log(`  Consensus: ${attackConsensus.consensusPercentage.toFixed(2)}%`);
  console.log(`  Forks: ${attackConsensus.forks.length}`);
  console.log(
    `  Impact on consensus: ${initialConsensus.consensusPercentage - attackConsensus.consensusPercentage}%`
  );

  // Stop the simulation
  networkSim.stop();

  // Analyze attack effectiveness
  const attackEffective =
    attackConsensus.consensusPercentage < initialConsensus.consensusPercentage * 0.9 ||
    attackConsensus.forks.length > initialConsensus.forks.length;

  console.log('\nAttack effectiveness analysis:');
  console.log(`  Attack was ${attackEffective ? 'EFFECTIVE' : 'INEFFECTIVE'}`);

  if (attackEffective) {
    console.log('  Recommendation: Implement additional Sybil resistance measures');
  } else {
    console.log('  Current Sybil resistance measures appear adequate');
  }
}

testSybilAttack();
```

These examples demonstrate how to use the network simulation utilities in the Wrfcoin Testing
Framework. You can adapt these patterns to test various network scenarios and consensus mechanisms
in your blockchain applications.
