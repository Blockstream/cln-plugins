# Virtual Channel Plugin

**Route Core Lightning payments across a trusted, unfunded link between two nodes.**

----

An experimental [Core Lightning](https://github.com/ElementsProject/lightning) plugin that forwards payment onions
between cooperating peers using custom messages.

## Overview

The plugin lets two cooperating CLN nodes act as adjacent hops in a payment route without a funded channel between them.
Both nodes store the same deterministic virtual short-channel ID, exchange the forwarded onion over a CLN custom
message, and coordinate the resulting preimage or failure.

Consider this route:

```text
             funded             virtual             funded
    A ───────────────── B1 ═════════════════ B2 ───────────────── C
                              trusted link
```

- `A` pays an invoice created by `C`.
- `B1` and `B2` run this plugin and have a virtual channel open with each other.
- `B1` and `B2` must be connected as Lightning peers so they can exchange custom messages.
- The sender supplies a route whose middle hop uses the virtual channel ID.

The plugin bridges HTLC processing, but it does not move funds or enforce a balance between `B1` and `B2`. The operators
are responsible for the trust relationship, accounting, liquidity, and any external settlement between those nodes.

## How it works

1. `A` sends a payment along a route containing the virtual channel between `B1` and `B2`.
2. `B1` intercepts the HTLC through the `htlc_accepted` hook and recognizes the outgoing virtual channel ID.
3. `B1` holds the HTLC and sends `B2` the next onion, payment hash, amount, and CLTV using a custom message.
4. `B2` calls CLN's `injectpaymentonion` RPC to continue the payment toward `C`.
5. `B2` returns either the payment preimage or a failure notification to `B1`.
6. `B1` resolves or fails the held HTLC. It also stops waiting when the outgoing CLTV height is reached.

Virtual channels and payment state are persisted in CLN's datastore under the `vchannel` namespace.

## Requirements

- Core Lightning node (developed under v25)
- The plugin installed and running on both virtual-channel peers
- A direct peer connection between those nodes
- Explicit trust and an external accounting or settlement arrangement between their operators

## Quick start

### 1. Build

From the repository root:

```bash
cargo build --release --package vchannel-plugin
```

The executable is created at:

```text
target/release/vchannel-plugin
```

### 2. Configure both CLN nodes

Add the plugin to the CLN configuration on both virtual-channel peers:

```ini
plugin=/absolute/path/to/cln-plugins/target/release/vchannel-plugin
vchannel-check-payment-sec=5
```

| Option                       | Required | Default | Description                                                                      |
|------------------------------|----------|---------|----------------------------------------------------------------------------------|
| `vchannel-check-payment-sec` | No       | `5`     | How often, in seconds, the held `htlc_accepted` hook checks for a payment result |

Restart both CLN nodes after changing their configuration.

### 3. Connect the peers

The nodes need a peer connection for custom-message delivery; a funded channel is not required:

```bash
lightning-cli connect <peer-node-id>@<host>:<port>
```

### 4. Open the virtual channel on both sides

On `B1`:

```bash
lightning-cli vch-open <B2-node-id>
```

On `B2`:

```bash
lightning-cli vch-open <B1-node-id>
```

Confirm that both nodes derived the same `virtual_channel_id`:

```bash
lightning-cli vch-list
```

The returned ID can then be used as the channel for the `B1 → B2` hop in an explicitly constructed route. The
plugin does not announce virtual channels to the network or add them to CLN's automatic route discovery.

## RPC reference

### `vch-open`

Creates a virtual channel for a peer, or reopens an existing one. The ID is deterministically derived from the two node
IDs, so both peers derive the same value.

```bash
lightning-cli vch-open <peer-node-id>
```

Named-parameter request:

```json
{
  "peer_id": "02..."
}
```

Returns an empty JSON value on success.

### `vch-close`

Marks a virtual channel as `Closed`. It does not delete its datastore entry.

```bash
lightning-cli vch-close <virtual-channel-id>
```

Named-parameter request:

```json
{
  "virtual_channel_id": "123x456x789"
}
```

Returns an empty JSON value on success.

### `vch-list`

Lists persisted virtual channels:

```bash
lightning-cli vch-list
```

Example response:

```json
[
  {
    "virtual_channel_id": "123x456x789",
    "peer_id": "02...",
    "status": "Opened"
  }
]
```

Possible channel states are `Opened` and `Closed`.

### `vch-status`

Reports whether the plugin is accepting new virtual-channel work:

```bash
lightning-cli vch-status
```

```json
{
  "status": "true"
}
```

> [!NOTE]
> `status` is currently returned as the string `"true"` or `"false"`, not as a JSON boolean.

### `vch-activate`

Activates virtual-channel processing:

```bash
lightning-cli vch-activate
```

The plugin starts active whenever it starts.

### `vch-deactivate`

Stops accepting new virtual-channel payments while allowing already-started work to finish:

```bash
lightning-cli vch-deactivate
```

Critical internal errors also deactivate the plugin as a safety measure. Inspect the CLN log before activating it again.

## Protocol

The plugin registers custom-message type `37914`. Its payload is a JSON-RPC request using one of three internal methods:

| Method            | Direction | Purpose                                |
|-------------------|-----------|----------------------------------------|
| `vchannel_pay`    | `B1 → B2` | Requests onion injection for a payment |
| `vchannel_paid`   | `B2 → B1` | Returns the payment preimage           |
| `vchannel_unpaid` | `B2 → B1` | Reports that onion injection failed    |

Messages are accepted only when the stored virtual channel belongs to the sending peer. Unknown message types and
methods are ignored.

### Payment request

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "vchannel_pay",
  "params": {
    "next_onion": "...",
    "payment_hash": "...",
    "amount_msat": 100000,
    "cltv": 800000,
    "virtual_channel_id": "123x456x789"
  }
}
```

### Success notification

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "vchannel_paid",
  "params": {
    "payment_hash": "...",
    "payment_preimage": "..."
  }
}
```

### Failure notification

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "vchannel_unpaid",
  "params": {
    "payment_hash": "..."
  }
}
```

These messages are an internal protocol and may change while the plugin is experimental.

## Operational behavior and limitations

- A payment hash is processed only once. Payment records persist in CLN's datastore, including across plugin restarts.
- Opening or closing a virtual channel changes local state only; run the corresponding command on both peers.
- Virtual channels are not gossiped and are unavailable to automatic route finding. Callers must construct a route
  explicitly, or it should be included into the invoice as route hint.
- A peer disconnect, custom-message failure, datastore failure, or CLN RPC failure can interrupt payment processing.
- The virtual-channel ID is a compact value derived from both public keys; it is an identifier, not a funding reference
  or cryptographic commitment.

## Testing

Build the release binary before running the integration suite:

```bash
cargo build --release --package vchannel-plugin
pytest vchannel-plugin/tests/test_plugin.py
```

The tests use the Core Lightning Python testing fixtures and create regtest nodes. Ensure the CLN test dependencies are
installed in your Python environment.

## Security model

This protocol trusts the remote virtual-channel peer to handle payment requests and report results correctly. It has no
on-chain funding transaction, commitment transaction, channel reserve, gossip announcement, or built-in mechanism for
reconciling value transferred between the two sides.

Deploy it only when that trust model is intentional and supported by operational controls outside this plugin.
