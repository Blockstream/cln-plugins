#!/usr/bin/env python3

import argparse
import os
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / ".venv" / "lib" / "python3.14" / "site-packages"))

from pyln.testing.db import SqliteDbProvider
from pyln.testing.utils import BitcoinD, LightningNode, NodeFactory, wait_for

PAYMENT_INTERVAL_SECS = 20


class _FakeNode:
    name = "regtest-harness"

    def get_closest_marker(self, *_):
        return None


class _FakeRequest:
    def __init__(self, workdir: str):
        self.node = _FakeNode()
        self._finalizers: list = []
        self._workdir = workdir

    def addfinalizer(self, fn):
        self._finalizers.append(fn)

    def run_finalizers(self):
        for fn in reversed(self._finalizers):
            try:
                fn()
            except Exception:
                pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--addr", default="0.0.0.0:9750", help="metrics listen address")
    parser.add_argument("--refresh", type=int, default=10, help="metrics refresh interval (secs)")
    parser.add_argument("--profile", default="debug", choices=["debug", "release"])
    args = parser.parse_args()

    plugin_path = ROOT / "target" / args.profile / "metrics-plugin"
    if not plugin_path.exists():
        print(f"ERROR: metrics-plugin not built at {plugin_path}")
        print("Run: cargo build -p metrics-plugin")
        sys.exit(1)

    workdir = tempfile.mkdtemp(prefix="metrics-regtest-")
    print(f"Work dir: {workdir}")

    executor = ThreadPoolExecutor(max_workers=16)
    fake_req = _FakeRequest(workdir)

    bitcoind = BitcoinD(bitcoin_dir=os.path.join(workdir, "bitcoin"))
    bitcoind.start()
    bitcoind.generate_block(101)
    print("bitcoind started")

    nf = NodeFactory(
        request=fake_req,
        testname="regtest-harness",
        bitcoind=bitcoind,
        executor=executor,
        directory=workdir,
        db_provider=SqliteDbProvider(workdir),
        node_cls=LightningNode,
        jsonschemas={},
    )

    base_opts = {"disable-plugin": "cln-grpc"}

    print("Starting l1 (metrics-plugin, routing node)")
    l1 = nf.get_node(
        unused_grpc_port=False,
        options={
            **base_opts,
            "plugin": str(plugin_path),
            "metrics-addr": args.addr,
            "metrics-refresh": str(args.refresh),
        },
    )

    print("Starting l2 (sender A)")
    l2 = nf.get_node(unused_grpc_port=False, options=base_opts)

    print("Starting l3 (receiver A)")
    l3 = nf.get_node(unused_grpc_port=False, options=base_opts)

    print("Starting l4 (sender B)")
    l4 = nf.get_node(unused_grpc_port=False, options=base_opts)

    print("Starting l5 (receiver B)")
    l5 = nf.get_node(unused_grpc_port=False, options=base_opts)

    print(f"\nl1 (router): {l1.info['id']}")
    print(f"l2 (sender A): {l2.info['id']}")
    print(f"l3 (receiver A): {l3.info['id']}")
    print(f"l4 (sender B): {l4.info['id']}")
    print(f"l5 (receiver B): {l5.info['id']}\n")

    print("Funding wallets")
    l1.fundwallet(12_000_000)
    l2.fundwallet(10_000_000)
    l4.fundwallet(10_000_000)

    print("l1 opens outbound channels to l3 and l5 (multifundchannel)")
    l1.connect(l3)
    l1.connect(l5)
    l1.rpc.multifundchannel([
        {"id": l3.info["id"], "amount": 5_000_000},
        {"id": l5.info["id"], "amount": 5_000_000},
    ])

    print("l2 and l4 open inbound channels to l1")
    l2.connect(l1)
    l2.rpc.fundchannel(l1.info["id"], 5_000_000)
    l4.connect(l1)
    l4.rpc.fundchannel(l1.info["id"], 5_000_000)

    print("Mining 6 blocks to confirm channels")
    bitcoind.generate_block(6)

    def channel_normal(node, peer_id):
        chs = node.rpc.listpeerchannels(peer_id)["channels"]
        return any(c.get("state") == "CHANNELD_NORMAL" for c in chs)

    wait_for(lambda: channel_normal(l1, l3.info["id"]))
    wait_for(lambda: channel_normal(l1, l5.info["id"]))
    wait_for(lambda: channel_normal(l2, l1.info["id"]))
    wait_for(lambda: channel_normal(l4, l1.info["id"]))
    print("Channels active.")

    wait_for(lambda: len(l1.rpc.listchannels()["channels"]) >= 8)
    print("Gossip propagated.")

    l1.rpc.setchannel("all", feebase=1000, feeppm=3000)
    print("Routing fees set: base=1000msat, ppm=3000")

    print(f"\nMetrics live at http://{args.addr}/metrics")
    print("Topology: l2 --> [l1] --> l3")
    print("l4 --> [l1] --> l5\n")

    payment_n = 0
    try:
        while True:
            time.sleep(PAYMENT_INTERVAL_SECS)
            bitcoind.generate_block(1)

            try:
                amt = 1_000_000_000 + payment_n * 100_000_000
                if payment_n % 2 == 0:
                    inv = l3.rpc.invoice(amt, f"pay_{payment_n}", f"payment {payment_n}")
                    l2.rpc.pay(inv["bolt11"])
                    print(f"[{payment_n}] l2 --[l1]--> l3  {amt // 1000} sat")
                else:
                    inv = l5.rpc.invoice(amt, f"pay_{payment_n}", f"payment {payment_n}")
                    l4.rpc.pay(inv["bolt11"])
                    print(f"[{payment_n}] l4 --[l1]--> l5  {amt // 1000} sat")
                payment_n += 1
            except Exception as e:
                print(f"[{payment_n}] payment failed: {e}")
                payment_n += 1

    except KeyboardInterrupt:
        print("\nShutting down!")
    finally:
        fake_req.run_finalizers()
        for node in nf.nodes:
            try:
                node.stop()
            except Exception:
                pass
        try:
            bitcoind.stop()
        except Exception:
            pass
        executor.shutdown(wait=False)
        print("Done!!!")


if __name__ == "__main__":
    main()
