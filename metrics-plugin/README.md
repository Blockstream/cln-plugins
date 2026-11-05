# Metrics - CLN Plugin

Core Lightning plugin that exposes node liquidity metrics via a Prometheus `/metrics` endpoint.

---

## Configuration

These are set as CLN plugin options (e.g. in `config` or passed via `--plugin`):

| Option                    | Required                                         | Default | Description                                                                  |
|---------------------------|---------------------------------------------------|---------|--------------------------------------------------------------------------------|
| `--metrics-addr`          | yes                                               | -       | Address and port to expose `/metrics` on (e.g. `127.0.0.1:9750`)             |
| `--metrics-refresh`       | no                                                | `30`    | How often (seconds) to refresh node data for gauge metrics. Minimum 5.       |
| `--metrics-disable-auth`  | no                                                | `false` | Serve `/metrics` without authentication. **Insecure** - local/dev use only.  |

The bearer token required to access `/metrics` is read from the `METRICS_TOKEN` environment
variable (not a plugin option), so it can be injected the way your deployment already manages
secrets - a Kubernetes `secretKeyRef`, a systemd `EnvironmentFile`, a Docker secret, etc. - rather
than requiring a file on disk.

The plugin refuses to start unless either `METRICS_TOKEN` or `--metrics-disable-auth` is set, so a
deployment can't accidentally expose `/metrics` without making an explicit choice. When
`--metrics-disable-auth` is used, the plugin logs a warning on startup.

The plugin rejects tokens shorter than 16 characters at startup (and an empty value), to catch
weak or placeholder values before `/metrics` is ever exposed. Generate a long, random token rather
than a memorable password, e.g.:

```
export METRICS_TOKEN=$(openssl rand -hex 32)
```

Example CLN config:

```
plugin=/path/to/metrics-plugin
metrics-addr=127.0.0.1:9750
metrics-refresh=15
```

Scrape it with:

```
curl -H "Authorization: Bearer $METRICS_TOKEN" http://127.0.0.1:9750/metrics
```

Prometheus scrape config equivalent:

```yaml
scrape_configs:
  - job_name: cln-metrics
    authorization:
      credentials: "<METRICS_TOKEN value>"
    static_configs:
      - targets: ["127.0.0.1:9750"]
```

---

## Metrics

### Node

| Metric                          | Type  | Description                       |
|---------------------------------|-------|-----------------------------------|
| `lightning_build_info`          | Gauge | Plugin version (label: `version`) |
| `lightning_node_blockheight`    | Gauge | Current Bitcoin blockheight       |
| `lightning_fees_collected_msat` | Gauge | Total routing fees collected      |

### Funds

| Metric                         | Type  | Description                         |
|--------------------------------|-------|-------------------------------------|
| `lightning_funds_output_msat`  | Gauge | On-chain balance                    |
| `lightning_funds_channel_msat` | Gauge | Balance held in channels (our side) |
| `lightning_funds_total_msat`   | Gauge | Total balance (on-chain + channels) |

### Liquidity

| Metric                          | Type  | Description                          |
|---------------------------------|-------|--------------------------------------|
| `lightning_total_outbound_msat` | Gauge | Total spendable across all channels  |
| `lightning_total_inbound_msat`  | Gauge | Total receivable across all channels |

### Channels (labels: `peer_id`, `scid`)

State (gauges):

| Metric                              | Type  | Description                    |
|-------------------------------------|-------|--------------------------------|
| `lightning_channel_balance_msat`    | Gauge | Our balance in this channel    |
| `lightning_channel_spendable_msat`  | Gauge | Currently spendable (outbound) |
| `lightning_channel_receivable_msat` | Gauge | Currently receivable (inbound) |
| `lightning_channel_capacity_msat`   | Gauge | Total channel capacity         |
| `lightning_channel_htlcs`           | Gauge | Active HTLCs                   |

Cumulative (counters, sourced from CLN's `listpeerchannels`):

| Metric                                           | Type    | Description                 |
|--------------------------------------------------|---------|-----------------------------|
| `lightning_channel_in_payments_offered_total`    | Counter | Incoming payments offered   |
| `lightning_channel_in_payments_fulfilled_total`  | Counter | Incoming payments fulfilled |
| `lightning_channel_in_msat_offered_total`        | Counter | Incoming msat offered       |
| `lightning_channel_in_msat_fulfilled_total`      | Counter | Incoming msat fulfilled     |
| `lightning_channel_out_payments_offered_total`   | Counter | Outgoing payments offered   |
| `lightning_channel_out_payments_fulfilled_total` | Counter | Outgoing payments fulfilled |
| `lightning_channel_out_msat_offered_total`       | Counter | Outgoing msat offered       |
| `lightning_channel_out_msat_fulfilled_total`     | Counter | Outgoing msat fulfilled     |

### Peers (label: `peer_id`)

| Metric                             | Type  | Description                                 |
|------------------------------------|-------|---------------------------------------------|
| `lightning_peer_connected`         | Gauge | `1` if peer is connected, `0` otherwise     |
| `lightning_peer_channels`          | Gauge | Number of channels with this peer           |
| `lightning_peer_channel_connected` | Gauge | Number of connected channels with this peer |

### Events (counters, reset on plugin restart)

| Metric                                  | Type    | Description                                              |
|-----------------------------------------|---------|----------------------------------------------------------|
| `lightning_forward_events_total`        | Counter | Forwarding events (label: `status`)                      |
| `lightning_channel_opened_total`        | Counter | Channels opened                                          |
| `lightning_htlc_accepted_total`         | Counter | HTLCs accepted                                           |
| `lightning_channel_state_changes_total` | Counter | Channel state changes (labels: `old_state`, `new_state`) |

---

## Subscriptions and hooks

**Subscriptions** (update counters in real time):

- `channel_opened`
- `forward_event`
- `channel_state_changed`

**Hook**:

- `htlc_accepted` - increments the counter and returns `continue`

---

## RPC

```
lightning-cli metrics-status
```

Returns current plugin status, number of tracked channels, and last seen blockheight.
