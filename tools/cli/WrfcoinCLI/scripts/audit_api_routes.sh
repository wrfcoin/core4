#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-http://localhost:4001/api}"
OUT_FILE="${2:-/tmp/wrfcoin_cli_route_audit.tsv}"

echo -e "METHOD\tPATH\tSTATUS" > "$OUT_FILE"

probe() {
  local method="$1"
  local path="$2"
  local body="${3:-}"
  local url="${BASE_URL}${path}"
  local status
  if [[ -n "$body" ]]; then
    status=$(curl -s -o /tmp/wrfcoin_cli_route_probe.out -w "%{http_code}" -X "$method" "$url" -H "content-type: application/json" --data "$body" || true)
  else
    status=$(curl -s -o /tmp/wrfcoin_cli_route_probe.out -w "%{http_code}" -X "$method" "$url" || true)
  fi
  echo -e "${method}\t${path}\t${status}" | tee -a "$OUT_FILE" >/dev/null
}

probe GET /health
probe GET /blockchain/status
probe GET /blockchain/blocks
probe GET /wallet/default
probe POST /wallet '{}'
probe GET /wallet/default/transactions
probe POST /transactions '{"from":"default","to":"wrf1x","amount":1,"fee":0.001,"signature":"pending"}'
probe GET /nft/abc
probe POST /nft '{"name":"x","description":"y","owner":"default","nft_type":"compute","metadata":{},"signature":"pending"}'
probe POST /node/mining/start '{"threads":2}'
probe GET /node/status
probe GET /bridge/status
probe POST /bridge/transfer '{"from_chain":"wrfcoin","to_chain":"ethereum","amount":1,"recipient":"x","signature":"pending"}'
probe GET /compute/networks/status
probe POST /compute/tasks '{"task_type":"weather-forecast","payload":{},"priority":1}'
probe GET /weather/stations/default
probe POST /weather/observations '{"station_id":"default","observation":{"parameter":"temperature","value":20},"signature":"pending"}'
probe POST /forecasts '{"title":"x","forecast_type":"weather","region":"global","data":{},"validity":24}'
probe GET /metrics/network
probe GET /metrics/blockchain
probe GET /iot/networks/status

echo "Route audit written to $OUT_FILE"
echo "Tip: 200/201/400/401/403/405 indicate route exists; 404 indicates mismatch."
