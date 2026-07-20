<div align="center">

# CLN Plugins

**Open-source plugins for Core Lightning nodes ⚡**

A growing collection of plugins for [Core Lightning](https://github.com/ElementsProject/lightning).

[Plugins](#plugins) · [Contributing](#contributing) · [Security](#security)

</div>

---

## Why this project?

Running a Core Lightning node often means connecting it to the rest of your infrastructure: monitoring, event pipelines,
dashboards, alerts, and more. This repository keeps those integrations small, composable, and open source.

Each plugin lives in its own workspace crate and can be built, configured, and run independently. There are three
plugins today—and the collection is designed to grow.

## Plugins

| Plugin                                 | What it does                                                      | 
|----------------------------------------|-------------------------------------------------------------------|
| [`event-plugin`](./event-plugin)       | Publishes CLN events and hook data to a message broker (RabbitMQ) |
| [`metrics-plugin`](./metrics-plugin)   | Exposes CLN node metrics to Prometheus                            |
| [`vchannel-plugin`](./vchannel-plugin) | Routes payments across a trusted, unfunded link between CLN nodes |

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

### Virtual channel plugin

Route a payment across two cooperating CLN nodes without a funded channel between them. The plugin relays the forwarded
onion over CLN custom messages and coordinates the resulting preimage or failure while the incoming HTLC is held.

Virtual channels are intended for explicitly trusted peers and external settlement arrangements. They are not announced
to the Lightning Network and must be included in a manually constructed route.

[Setup, RPC, protocol, and reference →](./vchannel-plugin/README.MD)

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
