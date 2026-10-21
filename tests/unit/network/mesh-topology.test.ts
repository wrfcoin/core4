/**
 * Mesh Network Topology Tests
 * Task 114.7: Node join/leave, partition handling, coordinator election
 *
 * Tests for distributed mesh network topology management
 * Target: 10 tests
 */

import { describe, it, expect, beforeEach, jest } from '@jest/globals';

enum NodeRole {
  COORDINATOR = 'coordinator',
  VALIDATOR = 'validator',
  RELAY = 'relay',
  OBSERVER = 'observer',
}

enum NodeStatus {
  ACTIVE = 'active',
  SYNCING = 'syncing',
  OFFLINE = 'offline',
  PARTITIONED = 'partitioned',
}

interface MeshNode {
  id: string;
  role: NodeRole;
  status: NodeStatus;
  peers: Set<string>;
  joinedAt: Date;
  lastHeartbeat: Date;
  latency: number;
}

class MockMeshNetwork {
  private nodes: Map<string, MeshNode> = new Map();
  private coordinatorId: string | null = null;

  async joinNetwork(
    nodeId: string,
    role: NodeRole = NodeRole.OBSERVER
  ): Promise<MeshNode> {
    if (this.nodes.has(nodeId)) {
      throw new Error('Node already exists');
    }

    const node: MeshNode = {
      id: nodeId,
      role,
      status: NodeStatus.SYNCING,
      peers: new Set(),
      joinedAt: new Date(),
      lastHeartbeat: new Date(),
      latency: Math.random() * 100,
    };

    this.nodes.set(nodeId, node);

    // Connect to existing nodes
    for (const [peerId, peer] of this.nodes) {
      if (peerId !== nodeId) {
        node.peers.add(peerId);
        peer.peers.add(nodeId);
      }
    }

    node.status = NodeStatus.ACTIVE;

    if (!this.coordinatorId && role === NodeRole.COORDINATOR) {
      this.coordinatorId = nodeId;
    }

    return node;
  }

  async leaveNetwork(nodeId: string): Promise<void> {
    const node = this.nodes.get(nodeId);
    if (!node) throw new Error('Node not found');

    // Remove from peers
    for (const peerId of node.peers) {
      const peer = this.nodes.get(peerId);
      if (peer) peer.peers.delete(nodeId);
    }

    this.nodes.delete(nodeId);

    if (this.coordinatorId === nodeId) {
      await this.electNewCoordinator();
    }
  }

  async electNewCoordinator(): Promise<string | null> {
    const validators = Array.from(this.nodes.values())
      .filter(
        n => n.role === NodeRole.VALIDATOR && n.status === NodeStatus.ACTIVE
      )
      .sort((a, b) => a.latency - b.latency);

    if (validators.length > 0) {
      this.coordinatorId = validators[0].id;
      validators[0].role = NodeRole.COORDINATOR;
      return this.coordinatorId;
    }

    this.coordinatorId = null;
    return null;
  }

  simulatePartition(nodeIds: string[]): void {
    for (const nodeId of nodeIds) {
      const node = this.nodes.get(nodeId);
      if (node) {
        node.status = NodeStatus.PARTITIONED;
        for (const peerId of node.peers) {
          if (!nodeIds.includes(peerId)) {
            const peer = this.nodes.get(peerId);
            peer?.peers.delete(nodeId);
          }
        }
      }
    }
  }

  healPartition(nodeIds: string[]): void {
    for (const nodeId of nodeIds) {
      const node = this.nodes.get(nodeId);
      if (node) {
        node.status = NodeStatus.ACTIVE;
        for (const [peerId, peer] of this.nodes) {
          if (peerId !== nodeId && !nodeIds.includes(peerId)) {
            node.peers.add(peerId);
            peer.peers.add(nodeId);
          }
        }
      }
    }
  }

  getNode(nodeId: string): MeshNode | undefined {
    return this.nodes.get(nodeId);
  }

  getCoordinatorId(): string | null {
    return this.coordinatorId;
  }

  getNodeCount(): number {
    return this.nodes.size;
  }

  getActiveNodes(): MeshNode[] {
    return Array.from(this.nodes.values()).filter(
      n => n.status === NodeStatus.ACTIVE
    );
  }
}

describe('Mesh Network Topology', () => {
  let network: MockMeshNetwork;

  beforeEach(() => {
    network = new MockMeshNetwork();
  });

  describe('Node Join/Leave', () => {
    it('should allow node to join network', async () => {
      const node = await network.joinNetwork('node-1', NodeRole.VALIDATOR);
      expect(node.status).toBe(NodeStatus.ACTIVE);
    });

    it('should connect new node to existing peers', async () => {
      await network.joinNetwork('node-1');
      await network.joinNetwork('node-2');
      const node3 = await network.joinNetwork('node-3');

      expect(node3.peers.size).toBe(2);
    });

    it('should remove node and update peers on leave', async () => {
      await network.joinNetwork('node-1');
      const node2 = await network.joinNetwork('node-2');
      await network.leaveNetwork('node-1');

      expect(node2.peers.has('node-1')).toBe(false);
      expect(network.getNodeCount()).toBe(1);
    });

    it('should reject duplicate node join', async () => {
      await network.joinNetwork('node-1');
      await expect(network.joinNetwork('node-1')).rejects.toThrow(
        'already exists'
      );
    });
  });

  describe('Coordinator Election', () => {
    it('should elect coordinator from validators', async () => {
      await network.joinNetwork('node-1', NodeRole.COORDINATOR);
      await network.joinNetwork('node-2', NodeRole.VALIDATOR);
      await network.leaveNetwork('node-1');

      expect(network.getCoordinatorId()).toBe('node-2');
    });

    it('should elect lowest latency validator as coordinator', async () => {
      const v1 = await network.joinNetwork('v1', NodeRole.VALIDATOR);
      const v2 = await network.joinNetwork('v2', NodeRole.VALIDATOR);

      v1.latency = 100;
      v2.latency = 50;

      await network.electNewCoordinator();
      expect(network.getCoordinatorId()).toBe('v2');
    });
  });

  describe('Partition Handling', () => {
    it('should mark partitioned nodes', async () => {
      await network.joinNetwork('node-1');
      await network.joinNetwork('node-2');

      network.simulatePartition(['node-1']);

      expect(network.getNode('node-1')?.status).toBe(NodeStatus.PARTITIONED);
    });

    it('should heal partition and restore connections', async () => {
      await network.joinNetwork('node-1');
      await network.joinNetwork('node-2');

      network.simulatePartition(['node-1']);
      network.healPartition(['node-1']);

      const node1 = network.getNode('node-1');
      expect(node1?.status).toBe(NodeStatus.ACTIVE);
      expect(node1?.peers.has('node-2')).toBe(true);
    });

    it('should only count active nodes', async () => {
      await network.joinNetwork('node-1');
      await network.joinNetwork('node-2');
      network.simulatePartition(['node-1']);

      expect(network.getActiveNodes().length).toBe(1);
    });
  });
});
