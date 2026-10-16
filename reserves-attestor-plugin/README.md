# Reserves attestor - CLN Plugin

Core Lightning plugin that produces a signed statement of how
much Bitcoin a node controls - on-chain and in channels - at a point in time.

---

## Why

A verifier needs an artifact that:

- states a node's balance at a point in time,
- was produced by the node itself
- signed with the node's own key, so it can be verified against the
  node's public Node ID without trusting whoever forwarded the report.

`lightning-cli signmessage`/`checkmessage` already give a node a way to sign
and verify arbitrary text with its own key ([BLIP-50](https://github.com/lightning/blips/blob/master/blip-0050.md)).
This plugin builds the statement text and signs it, so nobody has to hand-craft
the message or shell out to `signmessage` by hand.

---

## Configuration

| Option | Required | Description |
|---|---|---|
| `reserves-attestor-service-name` | no | Service name printed in the signed statement heading. Defaults to `Core Lightning`. |

```
plugin=/path/to/reserves-attestor-plugin
reserves-attestor-service-name=Example Service
```

If the option is omitted, the heading is `Core Lightning node balance
statement`.

---

## RPC

### `get-signed-balance`

Produces and signs a statement for this node, as of now.

```
lightning-cli get-signed-balance
```

The heading of the statement comes from the `reserves-attestor-service-name`
option (see Configuration) and is part of the signed text.

```json
{
  "statement_version": 1,
  "node_id": "02aabb...",
  "network": "bitcoin",
  "statement_time": "2026-09-17T17:42:00Z",
  "block_height": 912345,
  "onchain_balance_msat": 1000000000,
  "lightning_balance_msat": 250000000,
  "total_msat": 1250000000,
  "onchain_unconfirmed_msat": 0,
  "onchain_reserved_msat": 0,
  "onchain_immature_msat": 0,
  "lightning_pending_msat": 0,
  "statement": "-----BEGIN CLN SIGNED MESSAGE-----\nCore Lightning node balance statement\n\nStatement version:    1\nNode ID:              02aabb...\nNetwork:              bitcoin\nStatement time:       2026-09-17T17:42:00Z\nBlock height:         912345\n\nOn-chain balance:     1000000000 msats  (confirmed, spendable by this node)\nLightning balance:    250000000 msats  (this node's side of open channels)\nTotal under control:  1250000000 msats\n\nAccounting data:      none\n\nExcluded from totals:\n  - Unconfirmed on-chain:               0 msats\n  - Reserved on-chain UTXOs:            0 msats\n  - Immature on-chain, e.g. coinbase:   0 msats\n  - Pending channels, opening/closing:  0 msats\n-----BEGIN CLN SIGNATURE-----\n<zbase32 signature>\n-----END CLN SIGNATURE-----\n",
  "signature": "<zbase32 signature>"
}
```

`statement` is the plain-text artifact to hand to the verifier - save it as-is
(e.g. `lightning-cli get-signed-balance | jq -r .statement > statement.txt`).
The other fields are the same numbers, structured, for anything that wants to
consume them programmatically (a dashboard) without
re-parsing the text block.

`signature` is the zbase32 signature from `signmessage` over the message block
(the text inside the `statement` armor, byte-for-byte). The plugin runs its
own `checkmessage` round trip against that message/signature pair before the
RPC ever returns, and the call fails outright (instead of returning a
statement) if that check doesn't come back verified against the node's own
`node_id` - so any `statement`/`signature` pair you get back was already
independently re-checked by the node, not just asserted.

---

## Data sources

| Field | Source | Definition |
|---|---|---|
| On-chain balance | `listfunds` | Sum (in msat) of wallet outputs with `status = confirmed` and `reserved = false`. |
| Lightning balance | `listpeerchannels` | Sum of `to_us_msat` for channels with `state = CHANNELD_NORMAL`. |
| Total under control | - | On-chain balance + Lightning balance. Nothing else is ever added in. |

All amounts are msat - Lightning's native unit - so nothing has to be rounded
to fit; on-chain sat amounts are simply reported as `sat_amount * 1000`.

Caveated amounts are never added into any of the totals above; they are
reported as separate `Excluded from totals` lines (in the text) / their own
top-level `*_msat` fields (in the JSON). These fields are always present, with
zeros when there's nothing to caveat, so "missing" and "zero" are never
ambiguous and a parser
always sees the same shape:

| Note | Source |
|---|---|
| Unconfirmed on-chain | `listfunds` outputs with `status = unconfirmed` |
| Reserved on-chain UTXOs | `listfunds` outputs with `status = confirmed`, `reserved = true` (e.g. locked by an in-flight PSBT) |
| Immature on-chain | `listfunds` outputs with `status = immature` (e.g. unmatured coinbase) |
| Pending channels | `listpeerchannels` channels not in `CHANNELD_NORMAL` (opening or closing) |

---

## Signing and verification

The signed portion is exactly the text between the armor lines - the armor
itself is never part of what gets signed:

```
-----BEGIN CLN SIGNED MESSAGE-----
Core Lightning node balance statement

Statement version:    1
Node ID:              02aabb...
Network:              bitcoin
Statement time:       2026-09-17T17:42:00Z
Block height:         912345

On-chain balance:     1000000000 msats  (confirmed, spendable by this node)
Lightning balance:    250000000 msats  (this node's side of open channels)
Total under control:  1250000000 msats

Accounting data:      none

Excluded from totals:
  - Unconfirmed on-chain:               15000000 msats
  - Reserved on-chain UTXOs:            5000000 msats
  - Immature on-chain, e.g. coinbase:   2000000 msats
  - Pending channels, opening/closing:  8000000 msats
-----BEGIN CLN SIGNATURE-----
<zbase32 signature>
-----END CLN SIGNATURE-----
```

- `Statement version` is bumped whenever the layout changes, so a parser
  knows which rules apply instead of guessing from shape.
- `Statement time` is RFC 3339 (`%Y-%m-%dT%H:%M:%SZ`), always UTC.
- All amounts are msat, Lightning's native unit, so nothing is ever rounded
  to fit.
- `Excluded from totals` is always present, one line per field, with zeros
  when there's nothing to caveat - "missing" and "zero" are never ambiguous,
  and the section header already says these are excluded, so the per-line
  `(not counted)` suffix is gone.
- `Accounting data` is `none` for now; the line reserves a slot for a future
  CSV hash so adding one is a value change on that line, not a format change.

The plugin signs via `signmessage` (BLIP-50:
`zbase32(SigRec(SHA256(SHA256("Lightning Signed Message:" + msg))))`).

To verify, run `checkmessage` with the message and the zbase32 signature:

- `$SIGNATURE` is the `signature` field from the `get-signed-balance`
  response, used as-is.
- `$MESSAGE` is everything between (and not including) the `BEGIN`/`END`
  armor lines inside `statement`.

```
lightning-cli checkmessage "$MESSAGE" "$SIGNATURE" [node_id]
```

`checkmessage` returns `verified: true` when the signature matches, and
recovers the pubkey when `node_id` is omitted - useful to double check it
matches the `Node ID` printed in the statement itself. Passing `node_id` also
checks the recovered key matches it directly. The plugin already runs this
exact check once (server-side) before returning the statement - the RPC call
fails outright if it doesn't come back verified - so a mismatch on user's
side means the statement was altered in transit, not that the node's
signature was ever bad.
