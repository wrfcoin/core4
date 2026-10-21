# WXM Protocol: Weather Exchange Messaging

## Overview

The Weather Exchange Messaging (WXM) Protocol is Wrfcoin's communication standard for real-time
weather data exchange between nodes, sensors, and computational systems. WXM enables efficient,
secure, and standardized messaging across the entire weather network ecosystem.

## Protocol Architecture

### Message Structure

```json
{
  "wxm_version": "2.1.0",
  "message_id": "wxm_msg_12345678",
  "timestamp": "2024-01-15T10:30:00.000Z",
  "sender": {
    "node_id": "WRF_NODE_001",
    "node_type": "weather_station",
    "location": {
      "latitude": 40.7128,
      "longitude": -74.006
    },
    "signature": "0x1234567890abcdef..."
  },
  "recipient": {
    "node_id": "WRF_AGGREGATOR_01",
    "node_type": "data_aggregator"
  },
  "message_type": "data_submission",
  "priority": "normal",
  "payload": {
    // Message-specific data
  },
  "routing": {
    "hop_count": 0,
    "max_hops": 5,
    "route_history": []
  },
  "security": {
    "encryption": "aes-256-gcm",
    "integrity_hash": "sha256:abc123...",
    "nonce": "random_nonce_12345"
  }
}
```

## Message Types

### Data Submission Messages

```json
{
  "message_type": "data_submission",
  "payload": {
    "submission_id": "sub_001",
    "data_format": "wrfml",
    "data_version": "1.2.0",
    "measurements": {
      "temperature": {
        "value": 23.5,
        "unit": "celsius",
        "timestamp": "2024-01-15T10:30:00.000Z"
      },
      "humidity": {
        "value": 65.2,
        "unit": "percent",
        "timestamp": "2024-01-15T10:30:00.000Z"
      }
    },
    "quality_metrics": {
      "accuracy": 0.95,
      "completeness": 1.0,
      "timeliness": 0.98
    }
  }
}
```

### Validation Request Messages

```json
{
  "message_type": "validation_request",
  "payload": {
    "data_hash": "sha256:def456...",
    "validation_type": "consensus",
    "required_validators": 3,
    "timeout": 300,
    "incentive": {
      "amount": "10",
      "currency": "WRF"
    }
  }
}
```

### Consensus Messages

```json
{
  "message_type": "consensus_vote",
  "payload": {
    "proposal_id": "prop_123",
    "vote": "approve",
    "validator_stake": "1000",
    "confidence": 0.92,
    "reasoning": {
      "data_quality_score": 0.94,
      "source_reliability": 0.9,
      "cross_validation_results": [
        {
          "validator": "WRF_VAL_001",
          "score": 0.93
        }
      ]
    }
  }
}
```

### Alert Messages

```json
{
  "message_type": "weather_alert",
  "payload": {
    "alert_id": "alert_001",
    "severity": "high",
    "event_type": "extreme_temperature",
    "affected_area": {
      "type": "polygon",
      "coordinates": [
        [-74.02, 40.7],
        [-74.0, 40.72],
        [-74.02, 40.72],
        [-74.02, 40.7]
      ]
    },
    "conditions": {
      "temperature": 45.2,
      "duration": "4 hours",
      "trend": "increasing"
    },
    "recommendations": ["Seek air conditioning", "Stay hydrated", "Avoid outdoor activities"]
  }
}
```

## Transport Layers

### WebSocket Transport

```javascript
class WXMWebSocketTransport {
  constructor(endpoint, credentials) {
    this.ws = new WebSocket(endpoint);
    this.credentials = credentials;
    this.setupHandlers();
  }

  setupHandlers() {
    this.ws.onopen = () => {
      this.authenticate();
    };

    this.ws.onmessage = event => {
      const wxmMessage = JSON.parse(event.data);
      this.handleIncomingMessage(wxmMessage);
    };
  }

  sendMessage(messageType, payload, recipient) {
    const wxmMessage = {
      wxm_version: '2.1.0',
      message_id: this.generateMessageId(),
      timestamp: new Date().toISOString(),
      sender: this.getNodeInfo(),
      recipient: recipient,
      message_type: messageType,
      priority: 'normal',
      payload: payload,
      security: this.addSecurity(payload),
    };

    this.ws.send(JSON.stringify(wxmMessage));
  }

  addSecurity(payload) {
    const nonce = crypto.randomBytes(16).toString('hex');
    const dataString = JSON.stringify(payload);
    const hash = crypto.createHash('sha256').update(dataString).digest('hex');

    return {
      encryption: 'aes-256-gcm',
      integrity_hash: `sha256:${hash}`,
      nonce: nonce,
    };
  }
}
```

### HTTP/REST Transport

```javascript
class WXMHTTPTransport {
  constructor(baseUrl, apiKey) {
    this.baseUrl = baseUrl;
    this.apiKey = apiKey;
  }

  async sendMessage(messageType, payload, recipient) {
    const wxmMessage = this.createMessage(messageType, payload, recipient);

    const response = await fetch(`${this.baseUrl}/wxm/messages`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'WXM-Version': '2.1.0',
        Authorization: `Bearer ${this.apiKey}`,
      },
      body: JSON.stringify(wxmMessage),
    });

    return response.json();
  }

  async receiveMessages(filters = {}) {
    const params = new URLSearchParams(filters);
    const response = await fetch(`${this.baseUrl}/wxm/messages?${params}`, {
      headers: {
        'WXM-Version': '2.1.0',
        Authorization: `Bearer ${this.apiKey}`,
      },
    });

    return response.json();
  }
}
```

### P2P Mesh Transport

```javascript
class WXMPeerTransport {
  constructor(nodeId, port) {
    this.nodeId = nodeId;
    this.port = port;
    this.peers = new Map();
    this.messageBuffer = new Map();
  }

  async connectToPeer(peerId, address) {
    const peer = new WXMPeer(peerId, address);
    await peer.connect();
    this.peers.set(peerId, peer);
  }

  broadcastMessage(message) {
    for (const [peerId, peer] of this.peers) {
      if (peer.isConnected()) {
        peer.sendMessage(message);
      }
    }
  }

  routeMessage(message) {
    const { recipient, routing } = message;

    if (routing.hop_count >= routing.max_hops) {
      console.warn('Message exceeded max hops, dropping');
      return;
    }

    // Find best route to recipient
    const bestPeer = this.findOptimalRoute(recipient.node_id);

    if (bestPeer) {
      message.routing.hop_count++;
      message.routing.route_history.push(this.nodeId);
      bestPeer.sendMessage(message);
    }
  }
}
```

## Security Features

### Message Encryption

```javascript
class WXMSecurity {
  encryptMessage(payload, recipientPublicKey) {
    // Generate ephemeral key pair
    const ephemeralKeys = crypto.generateKeyPairSync('ec', {
      namedCurve: 'secp256k1',
    });

    // Derive shared secret
    const sharedSecret = crypto.diffieHellman({
      privateKey: ephemeralKeys.privateKey,
      publicKey: recipientPublicKey,
    });

    // Encrypt payload
    const cipher = crypto.createCipher('aes-256-gcm', sharedSecret);
    const encrypted = cipher.update(JSON.stringify(payload), 'utf8', 'hex');
    const final = cipher.final('hex');
    const tag = cipher.getAuthTag();

    return {
      ephemeral_public_key: ephemeralKeys.publicKey
        .export({
          type: 'spki',
          format: 'der',
        })
        .toString('hex'),
      encrypted_data: encrypted + final,
      auth_tag: tag.toString('hex'),
    };
  }

  decryptMessage(encryptedPayload, privateKey) {
    const ephemeralPublicKey = crypto.createPublicKey({
      key: Buffer.from(encryptedPayload.ephemeral_public_key, 'hex'),
      type: 'spki',
      format: 'der',
    });

    // Derive shared secret
    const sharedSecret = crypto.diffieHellman({
      privateKey: privateKey,
      publicKey: ephemeralPublicKey,
    });

    // Decrypt payload
    const decipher = crypto.createDecipher('aes-256-gcm', sharedSecret);
    decipher.setAuthTag(Buffer.from(encryptedPayload.auth_tag, 'hex'));

    const decrypted = decipher.update(encryptedPayload.encrypted_data, 'hex', 'utf8');
    const final = decipher.final('utf8');

    return JSON.parse(decrypted + final);
  }
}
```

## Quality of Service

### Priority Handling

```javascript
class WXMQoS {
  constructor() {
    this.messageQueues = {
      critical: [],
      high: [],
      normal: [],
      low: [],
    };
  }

  enqueueMessage(message) {
    const priority = message.priority || 'normal';
    this.messageQueues[priority].push(message);
  }

  processMessages() {
    // Process in priority order
    const priorities = ['critical', 'high', 'normal', 'low'];

    for (const priority of priorities) {
      while (this.messageQueues[priority].length > 0) {
        const message = this.messageQueues[priority].shift();
        this.deliverMessage(message);
      }
    }
  }

  calculateDeliveryMetrics(messageId) {
    return {
      latency: this.getMessageLatency(messageId),
      delivery_time: this.getDeliveryTime(messageId),
      hop_count: this.getHopCount(messageId),
      success_rate: this.getSuccessRate(),
    };
  }
}
```

## Network Discovery

### Node Discovery Protocol

```javascript
class WXMDiscovery {
  constructor(nodeId, capabilities) {
    this.nodeId = nodeId;
    this.capabilities = capabilities;
    this.knownNodes = new Map();
  }

  announcePresence() {
    const announcement = {
      wxm_version: '2.1.0',
      message_type: 'node_announcement',
      payload: {
        node_id: this.nodeId,
        node_type: this.capabilities.type,
        services: this.capabilities.services,
        location: this.capabilities.location,
        network_info: {
          supported_transports: ['websocket', 'http', 'p2p'],
          max_connections: 100,
          bandwidth_capacity: '10Mbps',
        },
      },
    };

    this.broadcastMessage(announcement);
  }

  handleNodeAnnouncement(message) {
    const nodeInfo = message.payload;
    this.knownNodes.set(nodeInfo.node_id, {
      ...nodeInfo,
      last_seen: Date.now(),
      connection_quality: this.assessConnectionQuality(nodeInfo),
    });
  }

  findNearbyNodes(location, radius) {
    const nearbyNodes = [];

    for (const [nodeId, nodeInfo] of this.knownNodes) {
      const distance = this.calculateDistance(location, nodeInfo.location);
      if (distance <= radius) {
        nearbyNodes.push({
          ...nodeInfo,
          distance: distance,
        });
      }
    }

    return nearbyNodes.sort((a, b) => a.distance - b.distance);
  }
}
```

## Use Cases

### Real-Time Weather Networks

- Instant data sharing between weather stations
- Rapid alert distribution for severe weather
- Coordinated sensor calibration

### IoT Device Communication

- Efficient messaging for resource-constrained devices
- Mesh networking for remote deployments
- Edge computing coordination

### Research Collaboration

- Data sharing between research institutions
- Collaborative model training
- Cross-validation networks

### Commercial Applications

- Weather service APIs
- Agricultural monitoring
- Smart city infrastructure

## Related Protocols

- **MQTT**: Message queuing for IoT
- **CoAP**: Constrained application protocol
- **LoRaWAN**: Long-range wide area network
- **XMPP**: Extensible messaging protocol
- **WebRTC**: Real-time communication
