# Event Collector - CLN Plugin

Core Lightning plugin that collects events and sends them to RabbitMQ

---

## Events handled

The plugin subscribes to the CLN notifications listed in `EVENT_PLUGIN_EVENTS`. Common event types are:

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

Set RabbitMQ URL with the plugin option:

```
EVENT_PLUGIN_EVENTS=connect,disconnect,invoice_creation,invoice_payment,channel_opened,channel_state_changed,forward_event,block_added
--rabbitmq-url=amqp://guest:guest@localhost:5672/%2f
--rabbitmq-exchange=some_exchange
--rabbitmq-queue=some_queue
```

`EVENT_PLUGIN_EVENTS` is required and must be set in the environment before CLN starts the plugin. Its comma-separated
value defines the notifications advertised to CLN during the plugin handshake.

If RabbitMQ is not available at **startup**, the plugin exits with an error and CLN will not load it.

If the connection drops **during operation**, the plugin keeps running but events cannot be published. Each failed
publish is logged at the `ERROR` level so operators can detect the data loss. No automatic reconnection is attempted —
restart the plugin to restore publishing.
