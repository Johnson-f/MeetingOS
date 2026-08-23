#!/usr/bin/env bash
set -euo pipefail

# Configure to any domain 
HOST="myserver"
DOMAIN="meet.tradstry.com"
DEPLOY_DIR="/opt/meeting-bot"
DOCKERHUB_USER="johnsonf"
DOCKERHUB_TOKEN="dckr_pat_Wsdhx9ODBqDM2nkOG1rxTsUZwvQ"
IMAGE="${DOCKERHUB_USER}/meeting-bot:latest"

echo "==> Deploying ${IMAGE} to ${HOST}"
echo ""

# --- get server IP ---
SERVER_IP=$(ssh "$HOST" "hostname -I | awk '{print \$1}'")

echo "=== DNS Setup ==="
echo "Add this A record at your domain provider:"
echo ""
echo "  Type: A"
echo "  Name: meet"
echo "  Value: ${SERVER_IP}"
echo "  TTL: 300 (or Auto)"
echo ""
echo "If using Cloudflare, set proxy status to 'DNS only' (grey cloud)"
echo "so Caddy can handle TLS directly."
echo ""
read -rp "Press Enter once DNS is configured (or to skip)..."
echo ""

# --- copy files to server ---
echo "==> Copying deployment files..."
ssh "$HOST" "mkdir -p ${DEPLOY_DIR}"
scp .env.production "$HOST:${DEPLOY_DIR}/.env.production"
scp docker-compose.yml "$HOST:${DEPLOY_DIR}/docker-compose.yml"
scp Caddyfile "$HOST:${DEPLOY_DIR}/Caddyfile"

# --- deploy on server ---
echo "==> Pulling image and starting services..."
ssh "$HOST" bash -s -- "$DEPLOY_DIR" "$DOMAIN" "$IMAGE" "$DOCKERHUB_TOKEN" "$DOCKERHUB_USER" <<'REMOTE'
set -euo pipefail

DEPLOY_DIR="$1"
DOMAIN="$2"
IMAGE="$3"

cd "$DEPLOY_DIR"

echo "Logging into Docker Hub..."
echo "$4" | docker login -u "$5" --password-stdin

echo "Pulling ${IMAGE}..."
docker pull "$IMAGE"

export DOMAIN="$DOMAIN"
export DOCKER_IMAGE="$IMAGE"

# stop existing services if running
docker compose down 2>/dev/null || true

# start services
docker compose up -d

echo ""
echo "Waiting for backend health check..."
for i in $(seq 1 30); do
  if docker compose ps backend --format '{{.Health}}' 2>/dev/null | grep -q "healthy"; then
    echo "Backend is healthy!"
    break
  fi
  if [ "$i" -eq 30 ]; then
    echo "Warning: backend hasn't become healthy yet. Check logs."
  fi
  sleep 2
done

echo ""
docker compose ps
REMOTE

echo ""
echo "==========================================="
echo " Deployed to https://${DOMAIN}"
echo "==========================================="
echo ""
echo "=== Useful Commands ==="
echo ""
echo "# SSH into the server"
echo "ssh ${HOST}"
echo ""
echo "# View all service status"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker compose ps'"
echo ""
echo "# Follow all logs in real time"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker compose logs -f'"
echo ""
echo "# Follow backend logs only"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker compose logs -f backend'"
echo ""
echo "# Follow caddy logs only"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker compose logs -f caddy'"
echo ""
echo "# Restart all services"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker compose restart'"
echo ""
echo "# Restart backend only"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker compose restart backend'"
echo ""
echo "# Stop everything"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker compose down'"
echo ""
echo "# Pull latest image and redeploy"
echo "ssh ${HOST} 'cd ${DEPLOY_DIR} && docker pull ${IMAGE} && docker compose up -d'"
echo ""
echo "# Check backend health"
echo "curl -s https://${DOMAIN}/health"
echo ""
