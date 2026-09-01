import json
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import pytest
from pyln.testing.fixtures import *  # noqa: F403,F401

PLUGIN_PATH = Path(__file__).parents[2] / "target" / "release" / "rpc-log-plugin"
GCS_ENDPOINT = "http://127.0.0.1:4443"

PROJECT = "test-project"

CHECKRUNE_BUCKET = "rpc-log-plugin-test"
COMMANDO_BUCKET = "rpc-log-plugin-commando-test"


def gcs_request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(
        f"{GCS_ENDPOINT}{path}",
        data=data,
        headers={"Content-Type": "application/json"},
        method=method,
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        payload = response.read()
        return json.loads(payload) if payload else None


@pytest.fixture(scope="module")
def fake_gcs():
    container_id = subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--detach",
            "--publish",
            "4443:4443",
            "fsouza/fake-gcs-server",
            "-scheme",
            "http",
            "-port",
            "4443",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()

    try:
        for _ in range(50):
            try:
                gcs_request("GET", f"/storage/v1/b?project={PROJECT}")
                break
            except (OSError, urllib.error.URLError):
                time.sleep(0.1)
        else:
            pytest.fail("fake-gcs-server did not become ready")

        for bucket in (CHECKRUNE_BUCKET, COMMANDO_BUCKET):
            gcs_request("POST", f"/storage/v1/b?project={PROJECT}", {"name": bucket})
        yield
    finally:
        subprocess.run(
            ["docker", "rm", "--force", container_id],
            check=False,
            capture_output=True,
        )


def only_log(bucket):
    objects = gcs_request("GET", f"/storage/v1/b/{bucket}/o").get("items", [])
    assert len(objects) == 1

    object_name = objects[0]["name"]
    encoded_name = urllib.parse.quote(object_name, safe="")
    log = gcs_request("GET", f"/storage/v1/b/{bucket}/o/{encoded_name}?alt=media")
    print("\n\nLog file content:", log, "\n\n")
    return log


def test_checkrune_is_uploaded(node_factory, fake_gcs):  # noqa: F811
    assert PLUGIN_PATH.exists(), f"Build the plugin first: {PLUGIN_PATH}"
    node = node_factory.get_node(
        options={
            "plugin": str(PLUGIN_PATH),
            "log-bucket": CHECKRUNE_BUCKET,
            "log-emulator-host": GCS_ENDPOINT,
        },
        start=True,
    )

    rune = node.rpc.createrune(restrictions=[["operator#IntegrationTest"]])["rune"]
    result = node.rpc.checkrune(rune=rune)
    assert result["valid"]

    log = only_log(CHECKRUNE_BUCKET)

    assert log["method"] == "checkrune"
    assert log["caller"] == "IntegrationTest"
    assert log["body"] == {"rune": "***"}


def test_invoice_through_commando_is_uploaded(node_factory, fake_gcs):  # noqa: F811
    assert PLUGIN_PATH.exists(), f"Build the plugin first: {PLUGIN_PATH}"
    plugin_options = {
        "plugin": str(PLUGIN_PATH),
        "log-bucket": COMMANDO_BUCKET,
        "log-emulator-host": GCS_ENDPOINT,
    }
    target, caller = node_factory.line_graph(
        2,
        fundchannel=False,
        opts=[plugin_options, {}],
    )

    rune = target.rpc.createrune(restrictions=[["operator#CommandoTest"]])["rune"]
    invoice_params = {
        "amount_msat": 1000,
        "label": "commando-integration-test",
        "description": "Created through commando",
    }
    result = caller.rpc.call(
        method="commando",
        payload={
            "peer_id": target.info["id"],
            "rune": rune,
            "method": "invoice",
            "params": invoice_params,
        },
    )
    assert "bolt11" in result

    log = only_log(COMMANDO_BUCKET)
    assert log["method"] == "checkrune"
    assert log["caller"] == "CommandoTest"
    assert log["body"] == {
        "nodeid": caller.info["id"],
        "rune": "***",
        "method": "invoice",
        "params": invoice_params,
    }
