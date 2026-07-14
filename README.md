<div align="center">

# CLN Plugins

**Open-source plugins for Core Lightning nodes ⚡**

A growing collection of plugins for [Core Lightning](https://github.com/ElementsProject/lightning).

[Plugins](#plugins) · [Quick start](#quick-start) · [Contributing](#contributing)

</div>

---

## Why this project?

Running a Core Lightning node often means connecting it to the rest of your infrastructure: monitoring, event pipelines,
dashboards, alerts, and more. This repository keeps those integrations small, composable, and open source.

Each plugin lives in its own workspace crate and can be built, configured, and run independently. There are two plugins
today—and the collection is designed to grow.

## Plugins

| Plugin                               | What it does                                                     | Integrates with |
|--------------------------------------|------------------------------------------------------------------|-----------------|
| [`event-plugin`](./event-plugin)     | Publishes CLN events and hook data to a message broker           | RabbitMQ        |
| [`metrics-plugin`](./metrics-plugin) | Exposes node, funds, liquidity, channel, peer, and event metrics | Prometheus      |

### Event plugin

Turn activity from your node into structured events. The plugin subscribes to CLN notifications, observes selected
hooks, encodes the events with Protocol Buffers, and publishes them to RabbitMQ.

Use it to feed event-driven services, analytics pipelines, audit systems, and operational tooling.

[Configuration and event reference →](./event-plugin/README.md)

### Metrics plugin

Give Prometheus a clear view into your node. The plugin serves a `/metrics` endpoint with gauges and counters for
balances, channel liquidity, peers, forwards, HTLCs, collected fees, and refresh health.

Use it as the foundation for dashboards, alerts, and day-to-day node monitoring.

[Configuration and metric reference →](./metrics-plugin/README.md)

## Quick start

### Prerequisites

- A working Core Lightning node
- A recent stable Rust toolchain
- Protocol Buffers compiler (`protoc`) to build `event-plugin`
- RabbitMQ for `event-plugin`, or Prometheus for `metrics-plugin`

### Build

Clone the repository and build every plugin in release mode:

```bash
git clone https://github.com/Blockstream/cln-plugins.git
cd cln-plugins
cargo build --release --workspace
```

The executables are created in `target/release/`:

```text
target/release/event-plugin
target/release/metrics-plugin
```

You can also build only the plugin you need:

```bash
cargo build --release --package metrics-plugin
```

## Development

Run the standard checks from the repository root:

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Contributing

Contributions are welcome—whether you are improving an existing integration, fixing documentation, or proposing an
entirely new plugin.

For a new plugin, aim for one clear responsibility, keep its configuration explicit, include a dedicated README, and add
it to the workspace and plugin table above. Please open an issue before starting a large change so the direction can be
discussed early.

## Security

Lightning plugins run alongside your node and may receive sensitive operational data. Review code and configuration
before deployment, bind network endpoints conservatively, protect broker credentials, and test changes outside
production first.

