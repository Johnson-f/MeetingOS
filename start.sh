#!/usr/bin/env bash
set -e

PORT=8080

# Start ngrok in background
echo "Starting ngrok tunnel on port $PORT..."
ngrok http "$PORT" --log=stdout --log-format=json > /tmp/ngrok.log 2>&1 &
NGROK_PID=$!

# Wait for ngrok to be ready and grab the public URL
NGROK_URL=""
for i in $(seq 1 20); do
    NGROK_URL=$(curl -s http://127.0.0.1:4040/api/tunnels 2>/dev/null | grep -o '"public_url":"https://[^"]*"' | head -1 | cut -d'"' -f4)
    if [ -n "$NGROK_URL" ]; then
        break
    fi
    sleep 0.5
done

if [ -z "$NGROK_URL" ]; then
    echo "WARNING: Could not get ngrok URL. Is ngrok authenticated?"
    echo "Run: ngrok config add-authtoken YOUR_TOKEN"
else
    echo ""
    echo "================================================"
    echo "  ngrok URL: $NGROK_URL"
    echo "  Webhook:   $NGROK_URL/api/v1/webhooks/recall"
    echo "================================================"
    echo ""
    echo "Set this webhook URL in your Recall AI dashboard."
    echo ""
fi

# Clean up ngrok on exit
cleanup() {
    echo "Stopping ngrok..."
    kill "$NGROK_PID" 2>/dev/null
    wait "$NGROK_PID" 2>/dev/null
}
trap cleanup EXIT

# Start the backend
RUST_BACKTRACE=full RUST_LOG=info PORT="$PORT" NGROK_URL="$NGROK_URL" cargo run
