# Main API Route Contract Checklist (Ordered)

> **Reconciled 2026-07-21:** This checklist records the historical TypeScript
> `main_api_server.ts` contract; that owner file is no longer present here. It is
> retained as migration context, not as evidence of a live route. The authoritative,
> code-checked native-node inventory is
> [`docs/security/RPC_SURFACE.md`](security/RPC_SURFACE.md).

This checklist defines the required `main_api_server.ts` route contracts in strict implementation order.

## 1. Chain Block Suite
Owner file: `backend/server/main_api_server.ts`

1. `GET /api/chain/block`
- Query: `{ "height"?: number, "hash"?: string }` (at least one required)
- Response `200`: `{ success, data: { height, hash, timestamp, transactions, miner, size, nonce } }`
- Response `400`: invalid/missing query values

2. `GET /api/chain/blocks`
- Query: `{ "limit"?: number, "startHeight"?: number }`
- Response `200`: `{ success, data: Block[] }`
- Response `400`: invalid query values

3. `GET /api/chain/pending-transactions`
- Response `200`: `{ success, data: Transaction[] }`

4. `POST /api/chain/export`
- Body: `{ "format"?: "json"|"csv", "startHeight"?: number, "endHeight"?: number }`
- Response `200`: `{ success, data: { format, exportedAt, blockCount, blocks } }`

5. `POST /api/chain/import`
- Body: `{ "data": unknown, "skipValidation"?: boolean }`
- Response `200`: `{ success, data: { imported, skipValidation, acceptedAt } }`
- Response `400`: missing `data`

## 2. Node Peer/Mine/Sync Control Suite
Owner file: `backend/server/main_api_server.ts`

1. `POST /api/node/mining/start`
- Body: `{ "threads"?: number, "address"?: string }`
- Response `200`: `{ success, data: { running, threads, address, startedAt } }`

2. `POST /api/node/mining/stop`
- Response `200`: `{ success, data: { running, stoppedAt } }`

3. `POST /api/node/peers/connect`
- Body: `{ "address": string }`
- Response `200`: `{ success, data: { connected, peer, connectedAt } }`
- Response `400`: missing/invalid `address`

4. `DELETE /api/node/peers/:peer`
- Response `200`: `{ success, data: { disconnected, peer, disconnectedAt } }`

5. `POST /api/node/peers/:peer/ban`
- Body: `{ "duration"?: number }`
- Response `200`: `{ success, data: { banned, peer, duration, bannedAt } }`

6. `DELETE /api/node/peers/:peer/ban`
- Response `200`: `{ success, data: { unbanned, peer, unbannedAt } }`

7. `GET /api/node/peers/banned`
- Response `200`: `{ success, data: [{ peer, duration, bannedAt }] }`

8. `GET /api/node/sync/status`
- Response `200`: `{ success, data: { syncing, progress, latestBlock, targetBlock, updatedAt } }`

9. `POST /api/node/sync`
- Response `200`: `{ success, data: { triggered, startedAt } }`

10. `GET /api/node/resources`
- Response `200`: `{ success, data: { cpuPercent, memoryPercent, diskPercent, uptimeSeconds } }`

11. `POST /api/node/reset`
- Body: `{ "blockchain"?: boolean, "wallet"?: boolean, "peers"?: boolean, "all"?: boolean }`
- Response `200`: `{ success, data: { reset, requested, resetAt } }`

## 3. Advanced NFT Workflow Suite
Owner file: `backend/server/main_api_server.ts`

1. `POST /api/nft/create`
- Body: `{ "title": string, "description": string, "computationType": string, "reward": number, "creator"?: string, "metadata"?: object }`
- Response `200`: `{ success, data: { id, status, createdAt, ... } }`
- Response `400`: required fields missing

2. `POST /api/nft/:id/submit`
- Body: `{ "result": string, "executor"?: string, "proof"?: object }`
- Response `200`: `{ success, data: { id, submissionId, submittedAt, status } }`

3. `GET /api/nft/categories`
- Response `200`: `{ success, data: [{ name, description }] }`

4. `POST /api/nft/categories`
- Body: `{ "name": string, "description"?: string }`
- Response `200`: `{ success, data: { name, description, createdAt } }`

5. `POST /api/nft/verifiers`
- Body: `{ "capacity"?: number, "supportedTypes"?: string[], "feePercentage"?: number }`
- Response `200`: `{ success, data: { id, capacity, supportedTypes, feePercentage, createdAt } }`

6. `GET /api/nft/verifiers`
- Query: `{ "computationType"?: string }`
- Response `200`: `{ success, data: Verifier[] }`

7. `POST /api/nft/:id/verify`
- Body: `{ "resultHash": string, "decision": "approve"|"reject", "notes"?: string }`
- Response `200`: `{ success, data: { id, verificationId, decision, verifiedAt } }`

## 4. Forecast Suite
Owner file: `backend/server/main_api_server.ts`

1. `POST /api/forecasts`
- Body: `{ "title": string, "forecastType": string, "region": string, "data": object, "validity"?: number, "confidence"?: number }`
- Response `200`: `{ success, data: { id, type: "single", createdAt, ... } }`

2. `POST /api/forecasts/ensemble`
- Body: `{ "title": string, "forecastType": string, "region": string, "members": object[], "validity"?: number }`
- Response `200`: `{ success, data: { id, type: "ensemble", membersCount, createdAt, ... } }`

3. `GET /api/forecasts`
- Query: `{ "forecastType"?: string, "region"?: string, "limit"?: number }`
- Response `200`: `{ success, data: Forecast[] }`

4. `GET /api/forecasts/:id`
- Response `200`: `{ success, data: Forecast }`
- Response `404`: forecast not found

5. `POST /api/forecasts/:id/validate`
- Body: `{ "observedData": object, "method"?: string }`
- Response `200`: `{ success, data: { id, method, score, validatedAt } }`

6. `POST /api/forecasts/compare`
- Body: `{ "ids": string[], "metrics"?: string[] }`
- Response `200`: `{ success, data: { ids, metrics, comparison, comparedAt } }`

## 5. Compute / Storage / AI/ML / IoT Suite
Owner file: `backend/server/main_api_server.ts`

1. `GET /api/compute/status`
- Response `200`: `{ success, data: { online, providers, queuedTasks, activeTasks } }`

2. `GET /api/compute/providers`
- Response `200`: `{ success, data: Provider[] }`

3. `POST /api/compute/providers/register`
- Body: `{ "providerType": string, "resources": object, "pricing": object }`
- Response `200`: `{ success, data: { id, providerType, registeredAt } }`

4. `POST /api/compute/tasks`
- Body: `{ "taskType": string, "payload": object, "priority"?: number }`
- Response `200`: `{ success, data: { taskId, status, createdAt } }`

5. `GET /api/compute/tasks/:id/status`
- Response `200`: `{ success, data: { id, status, progress, updatedAt } }`
- Response `404`: task not found

6. `GET /api/compute/tasks/:id/results`
- Response `200`: `{ success, data: { id, status, results } }`
- Response `404`: task not found

7. `GET /api/storage/status`
- Response `200`: `{ success, data: { online, providers, totalCapacityGb, usedCapacityGb } }`

8. `POST /api/storage/store`
- Body: `{ "data": string, "storageType": string, "redundancy"?: number }`
- Response `200`: `{ success, data: { cid, storageType, redundancy, storedAt } }`

9. `GET /api/aiml/status`
- Response `200`: `{ success, data: { online, modelsAvailable, queuedInferences } }`

10. `POST /api/aiml/inference`
- Body: `{ "modelId": string, "inputData": object, "parameters"?: object }`
- Response `200`: `{ success, data: { inferenceId, modelId, output, completedAt } }`

11. `GET /api/iot/status`
- Response `200`: `{ success, data: { online, networks, activeSensors, submissions24h } }`

12. `GET /api/iot/sensors/:sensorId`
- Response `200`: `{ success, data: { sensorId, status, lastReadingAt } }`

13. `POST /api/iot/readings`
- Body: `{ "sensor_id": string, "reading_type": string, "value": number, "timestamp"?: string, "signature"?: string }`
- Response `200`: `{ success, data: { accepted, sensorId, readingType, receivedAt } }`

## 6. Weather Submission Contract Consistency
Owner file: `backend/server/main_api_server.ts`

1. `POST /api/weather/submit`
- Keep middleware `weatherValidation` contract:
  - Body must include `{ stationId, sensorData[], amount }` and optional `timestamp`.
- Handler must no longer require direct top-level `temperature` / `humidity`.
- Response `200`: `{ success, data: WeatherReward }`.
