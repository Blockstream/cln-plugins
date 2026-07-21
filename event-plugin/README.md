# Event Collector - CLN Plugin

Core Lightning plugin that collects events and sends them to RabbitMQ

---

## Events handled

The plugin subscribes to a configurable list of CLN notifications. Common event types are:

- `connect` - A peer connected to the node
- `disconnect` - A peer disconnected from the node
- `invoice_creation` - A new invoice was created
- `invoice_payment` - An invoice was paid
- `channel_opened` - A new channel was opened
- `channel_state_changed` - A channel changed its state
- `forward_event` - A forwarding payment through the node
- `block_added` - A new blockchain block was added

## Hooks

There are 2 hooks:

- `peer_connected` - `hook.peer_connected` - Called before handshake with a peer finishes
- `htlc_accepted` - `hook.htlc_accepted` - Called when an incoming HTLC is received as part of a payment

Hooks allow you to intercept a CLN operation, handshake or HTLC acceptance, before it completes, publish the data to
RabbitMQ, and then return "continue" so that CLN can continue the operation

## Configuration

Set the event subscription source in the environment before starting CLN. For example:

```bash
export EVENT_PLUGIN_EVENTS="connect,disconnect,invoice_creation,invoice_payment,channel_opened,channel_state_changed,forward_event,block_added"
```

Configure RabbitMQ and the source kind through CLN plugin options. For example, when starting `lightningd`:

```bash
lightningd \
  --plugin=/path/to/event-plugin \
  --rabbitmq-url=amqp://guest:guest@localhost:5672/%2f \
  --rabbitmq-exchange=some_exchange \
  --rabbitmq-queue=some_queue \
  --source-kind=gateway
```

The list of notifications advertised to CLN during the plugin handshake is resolved from, in priority order:

1. `EVENT_PLUGIN_EVENTS` environment variable — a comma-separated list of event types.
2. The TOML config file pointed to by the `EVENT_PLUGIN_CONFIG` environment variable:

   ```toml
   [event-plugin]
   events_list = ["connect", "disconnect", "invoice_payment"]
   ```

3. A built-in default list covering the common notifications (connect/disconnect, invoices, channels, forwards,
   payments, coin movements, and more).

Because subscriptions are declared before CLN passes plugin options, `EVENT_PLUGIN_EVENTS` or `EVENT_PLUGIN_CONFIG`,
when used, must be present in the plugin process environment before CLN starts it. Wildcard subscriptions are not
supported.

If RabbitMQ is not available at **startup**, the plugin exits with an error and CLN will not load it.

If the connection drops **during operation**, the plugin keeps running but events cannot be published. Each failed
publish is logged at the `ERROR` level so operators can detect the data loss. No automatic reconnection is attempted —
restart the plugin to restore publishing.
