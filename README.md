# relay-rs

`relay-rs` is a centralized TCP/UDP relay system. A `relay-master` control plane
(Postgres-backed, mTLS gRPC, axum web panel with Discourse SSO) pushes desired
state to one or more `relay-node` data planes; the nodes terminate client
traffic and forward it to upstreams.

```
            ┌────────────────────┐
            │   relay-master     │  control plane
            │  axum panel :9090  │  (Postgres, JWT/SSO)
            │  gRPC mTLS :9443   │
            └─────────┬──────────┘
                      │  push desired segments / heartbeats
                      ▼
   ┌───────────┐  ┌───────────┐  ┌───────────┐
   │ relay-node│  │ relay-node│  │ relay-node│   data plane
   └───────────┘  └───────────┘  └───────────┘   (TCP/UDP relays)
```

## Quickstart

### 1. Install the master

```bash
curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-master.sh | bash
```

The installer downloads the `relay-master` binary, provisions Postgres (via
Docker, or reuses an existing `DATABASE_URL`), generates a CA, writes
`/etc/relay-rs/relay-master.env`, and starts `relay-master.service`.

### 2. Add a node

On the master host, mint a one-time enrollment token:

```bash
relay-master node-add --name edge-1
```

On the node host:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \
  --master https://master.example.com:9443 \
  --ca-b64 "$(cat /etc/relay-rs/relay-ca.b64)" \
  --enrollment-token <token-from-node-add> \
  --node-name edge-1
```

### 3. Open the panel

Visit `http://<master>:9090` (terminate TLS at a reverse proxy in production
and set `RELAY_PANEL_EXTERNAL_URL` accordingly).

## Architecture

- **`relay-master`** — control plane. Owns the source of truth in Postgres,
  exposes a gRPC mTLS API for nodes, and serves the web panel. The panel
  authenticates via Discourse SSO and signs sessions with a JWT secret.
- **`relay-node`** — data plane. Authenticates to the master with an mTLS
  client cert obtained during enrollment, syncs desired state, and binds
  TCP/UDP listeners that relay traffic to upstream targets.
- **`relay-proto`** — shared protobuf / tonic stubs used by both binaries.

## Configuration

`relay-master` reads from `/etc/relay-rs/relay-master.env`:

| Variable | Purpose |
| --- | --- |
| `DATABASE_URL` | Postgres connection string (required) |
| `RELAY_MASTER_CA_DIR` | Directory holding the CA + server cert |
| `RELAY_MASTER_TOKEN_DIR` | Directory for enrollment tokens |
| `RELAY_MASTER_LISTEN` | gRPC mTLS listen addr (e.g. `0.0.0.0:9443`) |
| `RELAY_MASTER_HOSTNAME` | Comma-separated SAN list for the server cert |
| `RELAY_PANEL_LISTEN` | HTTP listen addr for the panel (e.g. `0.0.0.0:9090`) |
| `RELAY_PANEL_EXTERNAL_URL` | Public URL the panel is reached at |
| `RELAY_PANEL_JWT_SECRET` | 32-byte hex secret used to sign panel sessions |
| `RELAY_MASTER_PUBLIC_URL` | Optional; overrides what the panel advertises to nodes |

`relay-node` reads from `/etc/relay-rs/relay-node.env`:

| Variable | Purpose |
| --- | --- |
| `MASTER_ADDR` | gRPC URL of the master (e.g. `https://master:9443`) |
| `NODE_STATE_DIR` | Where the node keeps its cert + state |
| `MASTER_CA_PEM_B64` | (register-only) base64 PEM of the master CA |
| `ENROLLMENT_TOKEN` | (register-only) one-time token from `node-add` |
| `NODE_NAME` | (register-only) human-readable node name |

## Development

```bash
# build everything
cargo build --workspace

# clippy on the production crates
cargo clippy -p relay-proto -p relay-master -p relay-node -- -D warnings

# tests
cargo test --workspace

# apply the SQL migrations against a local Postgres
for f in crates/master/migrations/*.sql; do
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$f"
done

# frontend (Bun)
cd panel && bun install && bun run build
```

End-to-end smoke test (spins up Postgres in Docker, runs master + node, asserts
TCP forwarding):

```bash
bash scripts/smoke.sh
```

## License

MIT — see [LICENSE](LICENSE) if present, otherwise the `license = "MIT"` field
in `Cargo.toml`.
