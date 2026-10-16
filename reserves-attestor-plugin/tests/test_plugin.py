from pathlib import Path

from pyln.testing.fixtures import *  # noqa: F403,F401


PLUGIN_PATH = (
    Path(__file__).parents[2] / "target" / "release" / "reserves-attestor-plugin"
)
BEGIN_MESSAGE = "-----BEGIN CLN SIGNED MESSAGE-----\n"
BEGIN_SIGNATURE = "\n-----BEGIN CLN SIGNATURE-----\n"
END_SIGNATURE = "\n-----END CLN SIGNATURE-----\n"


def start_node(node_factory, **options):
    assert PLUGIN_PATH.exists(), f"Build the plugin first: {PLUGIN_PATH}"
    return node_factory.get_node(
        options={"plugin": str(PLUGIN_PATH), **options},
        start=True,
    )


def verify_statement(node, response):
    statement = response["statement"]
    assert statement.startswith(BEGIN_MESSAGE)

    message, signature_block = statement.removeprefix(BEGIN_MESSAGE).split(
        BEGIN_SIGNATURE, 1
    )
    signature, trailing = signature_block.split(END_SIGNATURE, 1)

    assert not trailing
    assert signature == response["signature"]

    result = node.rpc.call(
        method="checkmessage",
        payload={
            "message": message,
            "zbase": signature,
            "pubkey": response["node_id"],
        },
    )
    assert result["verified"]
    return message


def test_get_signed_balance(node_factory):  # noqa: F811
    node = start_node(node_factory)
    response = node.rpc.call("get-signed-balance")

    print(response["statement"])

    assert response["statement_version"] == 1
    assert response["node_id"] == node.info["id"]
    assert response["network"] == node.info["network"]
    assert response["block_height"] == node.info["blockheight"]
    assert response["total_msat"] == (
        response["onchain_balance_msat"] + response["lightning_balance_msat"]
    )
    assert "block_hash" not in response

    message = verify_statement(node, response)
    assert message.startswith("Core Lightning node balance statement\n")
    assert f"Node ID:              {node.info['id']}" in message
    assert f"Network:              {node.info['network']}" in message
    assert f"Block height:         {node.info['blockheight']}" in message


def test_custom_service_name(node_factory):  # noqa: F811
    node = start_node(
        node_factory,
        **{"reserves-attestor-service-name": "Example Service"},
    )
    response = node.rpc.call("get-signed-balance")

    message = verify_statement(node, response)
    assert message.startswith("Example Service node balance statement\n")
