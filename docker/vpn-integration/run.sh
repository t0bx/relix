#!/usr/bin/env bash
# Runs the relix-vpn WireGuard system tests against a real tunnel between two containers.
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
network=relix-vpn-test
server=relix-vpn-peer
keys="$(mktemp -d)"
trap 'docker rm -f "$server" >/dev/null 2>&1 || true; docker network rm "$network" >/dev/null 2>&1 || true; rm -rf "$keys"' EXIT

docker network create "$network" >/dev/null
docker run -d --name "$server" --network "$network" --cap-add NET_ADMIN \
  -v "$keys":/keys alpine:3.20 sh -c '
    apk add -q wireguard-tools iproute2 socat >/dev/null
    wg genkey | tee /keys/server.key | wg pubkey > /keys/server.pub
    wg genkey | tee /keys/client.key | wg pubkey > /keys/client.pub
    ip link add wg0 type wireguard
    wg set wg0 listen-port 51820 private-key /keys/server.key \
      peer "$(cat /keys/client.pub)" allowed-ips 10.99.0.2/32
    ip address add 10.99.0.1/24 dev wg0
    ip link set wg0 up
    touch /keys/ready
    exec socat TCP-LISTEN:7000,bind=10.99.0.1,fork,reuseaddr EXEC:cat' >/dev/null

for _ in $(seq 1 60); do [ -f "$keys/ready" ] && break; sleep 1; done
cat > "$keys/relixwg.conf" <<CONF
[Interface]
PrivateKey = $(cat "$keys/client.key")
Address = 10.99.0.2/24

[Peer]
PublicKey = $(cat "$keys/server.pub")
Endpoint = $server:51820
AllowedIPs = 10.99.0.0/24
PersistentKeepalive = 5
CONF
chmod 600 "$keys/relixwg.conf"

docker run --rm --network "$network" --cap-add NET_ADMIN \
  -v "$repo":/src:ro -v "$keys":/wg:ro \
  -v relix-linux-target:/target -v relix-linux-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/target -e RELIX_VPN_INTEGRATION=1 \
  -e RELIX_TEST_WG_CONFIG=/etc/wireguard/relixwg.conf -e RELIX_TEST_WG_PROBE=10.99.0.1:7000 \
  -w /src rust:1.97-bookworm bash -c '
    apt-get update -qq >/dev/null
    apt-get install -y -qq wireguard-tools iproute2 pkexec >/dev/null
    install -m 600 /wg/relixwg.conf /etc/wireguard/relixwg.conf
    cargo test -p relix-vpn --locked --test wireguard_system -- --test-threads=1'
