import time
from pathlib import Path
import pytest
from pyln.testing.fixtures import *
import logging

"""Path to the compiled cln-vchannel binary"""
_PLUGIN_PATH = Path(__file__).parents[2] / "target" / "release" / "vchannel-plugin"


@pytest.fixture
def plugin_path():
    if not _PLUGIN_PATH.exists():
        pytest.skip(f"Plugin not built: {_PLUGIN_PATH}")
    return str(_PLUGIN_PATH)


def test_plugin_is_active(node_factory, plugin_path):
    l1 = node_factory.get_node(options={"plugin": plugin_path}, start=True)

    assert l1.rpc.vch_status()["status"] == "true"


def test_plugin_deactivate(node_factory, plugin_path):
    l1 = node_factory.get_node(options={"plugin": plugin_path}, start=True)

    l1.rpc.vch_deactivate()
    assert l1.rpc.vch_status()["status"] == "false"


def test_plugin_activate(node_factory, plugin_path):
    l1 = node_factory.get_node(options={"plugin": plugin_path}, start=True)

    l1.rpc.vch_deactivate()
    assert l1.rpc.vch_status()["status"] == "false"

    l1.rpc.vch_activate()
    assert l1.rpc.vch_status()["status"] == "true"


def test_virtual_channel_open_and_close(node_factory, plugin_path):
    l1 = node_factory.get_node(options={"plugin": plugin_path}, start=True)
    l2 = node_factory.get_node(options={}, start=True)

    # get peer id of l2
    peer_id = l2.rpc.getinfo()["id"]

    # open virtual channel to l2
    l1.rpc.vch_open(peer_id=peer_id)

    # get list of virtual channels
    channels_list_opened = l1.rpc.vch_list()

    assert len(channels_list_opened) == 1
    assert channels_list_opened[0]["peer_id"] == peer_id
    assert channels_list_opened[0]["status"] == "Opened"

    # close virtual channel
    l1.rpc.vch_close(virtual_channel_id=channels_list_opened[0]["virtual_channel_id"])

    # get new list of virtual channels
    channels_list_closed = l1.rpc.vch_list()

    assert len(channels_list_closed) == 1
    assert channels_list_closed[0]["peer_id"] == peer_id
    assert channels_list_closed[0]["status"] == "Closed"
    assert channels_list_closed[0]["virtual_channel_id"] == channels_list_opened[0]["virtual_channel_id"]


def test_virtual_channel_list(node_factory, plugin_path):
    l1 = node_factory.get_node(options={"plugin": plugin_path}, start=True)
    l2 = node_factory.get_node(options={}, start=True)
    l3 = node_factory.get_node(options={}, start=True)

    # get peer ids of l2 and l3
    peer_id_l2 = l2.rpc.getinfo()["id"]
    peer_id_l3 = l3.rpc.getinfo()["id"]

    # open virtual channels to them
    l1.rpc.vch_open(peer_id=peer_id_l2)
    l1.rpc.vch_open(peer_id=peer_id_l3)

    # get list of virtual channels
    channels_list_opened = l1.rpc.vch_list()

    assert len(channels_list_opened) == 2

    # we can not predict the ordering so let's compare the dictionaries
    received = {
        channels_list_opened[0]["peer_id"]: channels_list_opened[0]["status"],
        channels_list_opened[1]["peer_id"]: channels_list_opened[1]["status"],
    }

    should_be = {
        peer_id_l2: "Opened",
        peer_id_l3: "Opened",
    }

    assert received == should_be


def test_virtual_channel_open_twice(node_factory, plugin_path):
    l1 = node_factory.get_node(options={"plugin": plugin_path}, start=True)
    l2 = node_factory.get_node(options={}, start=True)

    # get peer id of l2
    peer_id = l2.rpc.getinfo()["id"]

    # open virtual channel to l2
    l1.rpc.vch_open(peer_id=peer_id)

    # list current virtual channels
    channels_list_opened_1 = l1.rpc.vch_list()

    assert len(channels_list_opened_1) == 1
    assert channels_list_opened_1[0]["peer_id"] == peer_id
    assert channels_list_opened_1[0]["status"] == "Opened"

    # re-open virtual channel to the same peer
    l1.rpc.vch_open(peer_id=peer_id)

    # list current virtual channels (we expect that status hasn't been changed)
    channels_list_opened_2 = l1.rpc.vch_list()
    assert len(channels_list_opened_2) == 1
    assert channels_list_opened_2[0]["peer_id"] == peer_id
    assert channels_list_opened_2[0]["status"] == "Opened"
    assert channels_list_opened_2[0]["virtual_channel_id"] == channels_list_opened_1[0]["virtual_channel_id"]

    # close virtual channel
    l1.rpc.vch_close(virtual_channel_id=channels_list_opened_1[0]["virtual_channel_id"])

    # list current virtual channels (we expect that status has been changed)
    channels_list_closed_1 = l1.rpc.vch_list()

    assert len(channels_list_closed_1) == 1
    assert channels_list_closed_1[0]["peer_id"] == peer_id
    assert channels_list_closed_1[0]["status"] == "Closed"
    assert channels_list_closed_1[0]["virtual_channel_id"] == channels_list_opened_2[0]["virtual_channel_id"]

    # re-open virtual channel to l2
    l1.rpc.vch_open(peer_id=peer_id)

    # list current virtual channels (we expect that status has been changed)
    channels_list_opened_3 = l1.rpc.vch_list()
    assert len(channels_list_opened_3) == 1
    assert channels_list_opened_3[0]["peer_id"] == peer_id
    assert channels_list_opened_3[0]["status"] == "Opened"
    assert channels_list_opened_3[0]["virtual_channel_id"] == channels_list_closed_1[0]["virtual_channel_id"]


def test_virtual_channels_has_same_id(node_factory, plugin_path):
    l1 = node_factory.get_node(options={"plugin": plugin_path}, start=True)
    l2 = node_factory.get_node(options={"plugin": plugin_path}, start=True)

    # get peer ids of l1 and l2
    peer_id_l1 = l1.rpc.getinfo()["id"]
    peer_id_l2 = l2.rpc.getinfo()["id"]

    # open virtual channel between l1 and l2
    l1.rpc.vch_open(peer_id=peer_id_l2)
    l2.rpc.vch_open(peer_id=peer_id_l1)

    # List current virtual channels on both nodes
    channels_list_l1 = l1.rpc.vch_list()
    channels_list_l2 = l2.rpc.vch_list()

    assert len(channels_list_l1) == 1
    assert channels_list_l1[0]["peer_id"] == peer_id_l2
    assert channels_list_l1[0]["status"] == "Opened"

    assert len(channels_list_l2) == 1
    assert channels_list_l2[0]["peer_id"] == peer_id_l1
    assert channels_list_l2[0]["status"] == "Opened"

    # Check that on both nodes we have the same virtual channel id
    assert channels_list_l1[0]["virtual_channel_id"] == channels_list_l2[0]["virtual_channel_id"]


def test_payment_success(node_factory, plugin_path):
    b1 = node_factory.get_node(options={"plugin": plugin_path, "fee-base": 0, "fee-per-satoshi": 0}, start=True)
    b2 = node_factory.get_node(options={"plugin": plugin_path, "fee-base": 0, "fee-per-satoshi": 0}, start=True)
    a = node_factory.get_node(options={"fee-base": 0, "fee-per-satoshi": 0}, start=True)
    c = node_factory.get_node(options={"fee-base": 0, "fee-per-satoshi": 0}, start=True)

    # Get all peer ids
    peer_id_a = a.rpc.getinfo()["id"]
    peer_id_b1 = b1.rpc.getinfo()["id"]
    peer_id_b2 = b2.rpc.getinfo()["id"]
    peer_id_c = c.rpc.getinfo()["id"]

    # Create topology A-B1--B2-C, where -- denotes a virtual channel
    node_factory.join_nodes([a, b1], fundchannel=True, wait_for_announce=True)
    node_factory.join_nodes([b2, c], fundchannel=True, wait_for_announce=True)
    node_factory.join_nodes([b1, b2], fundchannel=False)

    b1.rpc.vch_open(peer_id=peer_id_b2)
    b2.rpc.vch_open(peer_id=peer_id_b1)

    # Get channels
    channel_a_b1 = a.rpc.listpeerchannels(peer_id=peer_id_b1)["channels"][0]
    channel_b2_c = c.rpc.listpeerchannels(peer_id=peer_id_b2)["channels"][0]

    # Get virtual channel id
    virtual_channel_id = b1.rpc.vch_list()[0]["virtual_channel_id"]

    # Get initial balances of A (in A-B1 channel) and c (in B2-C channel)
    a_initial_balance = channel_a_b1["to_us_msat"]
    c_initial_balance = channel_b2_c["to_us_msat"]
    b1_initial_balance = channel_a_b1["total_msat"] - a_initial_balance
    b2_initial_balance = channel_b2_c["total_msat"] - c_initial_balance

    # Amount to transfer
    amt = 100000

    # Create invoice
    bolt11 = c.rpc.invoice(amount_msat=amt, label="Test", description="A payment via virtual channel")["bolt11"]
    bolt11_dec = a.rpc.decode(bolt11)

    # Prepare custom route hints, to include our virtual channel
    route = [
        {"amount_msat": amt, "id": peer_id_b1, "delay": 100, "channel": channel_a_b1["short_channel_id"]},
        {"amount_msat": amt, "id": peer_id_b2, "delay": 80, "channel": virtual_channel_id},
        {"amount_msat": amt, "id": peer_id_c, "delay": 50, "channel": channel_b2_c["short_channel_id"]},
    ]

    # Send payment from A to C
    a.rpc.sendpay(
        route,
        bolt11_dec["payment_hash"],
        bolt11=bolt11,
        payment_secret=bolt11_dec["payment_secret"],
    )

    # Wait until payment succeed from C's perspective
    assert c.rpc.waitinvoice(label="Test")["payment_preimage"] != ""

    # Wait a little until A also receives a preimage
    time.sleep(10)

    # Get channels (again, updated)
    channel_a_b1 = a.rpc.listpeerchannels(peer_id=peer_id_b1)["channels"][0]
    channel_b2_c = c.rpc.listpeerchannels(peer_id=peer_id_b2)["channels"][0]

    # Get current balances
    a_current_balance = channel_a_b1["to_us_msat"]
    c_current_balance = channel_b2_c["to_us_msat"]
    b1_current_balance = channel_a_b1["total_msat"] - a_current_balance
    b2_current_balance = channel_b2_c["total_msat"] - c_current_balance

    # Verify that balances changed from perspective of payer (A) and payee (C)
    assert a_initial_balance - amt == a_current_balance
    assert c_initial_balance + amt == c_current_balance

    # Make sure that B1's balance increased and B2's balance decreased
    assert b1_initial_balance + amt == b1_current_balance
    assert b2_initial_balance - amt == b2_current_balance
