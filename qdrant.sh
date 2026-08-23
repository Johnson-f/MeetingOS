#!/usr/bin/env bash
set -euo pipefail

HOST="myserver"
CONTAINER="qdrant"
GRPC_PORT=6334
HTTP_PORT=6333

read -rsp "Set a Qdrant API key (password): " API_KEY
echo

echo "Setting up Qdrant on ${HOST}..."

ssh "$HOST" bash -s -- "$CONTAINER" "$GRPC_PORT" "$HTTP_PORT" "$API_KEY" <<'REMOTE'
set -euo pipefail

CONTAINER="$1"
GRPC_PORT="$2"
HTTP_PORT="$3"
API_KEY="$4"

echo "Pulling latest Qdrant image..."
docker pull qdrant/qdrant:latest

# stop existing container if running
if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER}$"; then
  echo "Removing existing container..."
  docker stop "$CONTAINER" 2>/dev/null || true
  docker rm "$CONTAINER" 2>/dev/null || true
fi

echo "Starting Qdrant with gRPC on port ${GRPC_PORT}..."
docker run -d \
  --name "$CONTAINER" \
  --restart unless-stopped \
  -p "${GRPC_PORT}:6334" \
  -p "${HTTP_PORT}:6333" \
  -v qdrant_data:/qdrant/storage \
  -e QDRANT__SERVICE__API_KEY="$API_KEY" \
  -e QDRANT__SERVICE__GRPC_PORT=6334 \
  -e QDRANT__SERVICE__ENABLE_GRPC=true \
  qdrant/qdrant:latest

echo "Waiting for Qdrant to start..."
for i in $(seq 1 10); do
  if curl -sf http://localhost:${HTTP_PORT}/healthz > /dev/null 2>&1; then
    echo "Qdrant is healthy."
    break
  fi
  sleep 1
done
REMOTE

HOSTNAME=$(ssh "$HOST" "hostname -f")

echo ""
echo "=== Qdrant is running ==="
echo "URL:     https://${HOSTNAME}:${GRPC_PORT}"
echo "API Key: ${API_KEY}"
echo ""
echo "For .env.production:"
echo "  QDRANT_URL=https://${HOSTNAME}:${GRPC_PORT}"
echo "  QDRANT_API_KEY=${API_KEY}"
