"""End-to-end integration tests: libbacnet Python client against a real
bacpypes3 BACnet/IP device running on localhost.

Each test spins up a bacpypes3 ``NormalApplication`` on a random loopback
port, runs a short asyncio scenario, then tears the server down.

These tests require the ``libbacnet`` native extension to be built
(``maturin develop``) and ``bacpypes3`` to be installed.

Run with::

    pytest tests/test_e2e.py -v
"""

from __future__ import annotations

import asyncio

import pytest

libbacnet = pytest.importorskip("libbacnet", reason="libbacnet native extension not built")
bacpypes3 = pytest.importorskip("bacpypes3", reason="bacpypes3 not installed")

from bacpypes3.ipv4.app import NormalApplication  # noqa: E402
from bacpypes3.local.analog import AnalogInputObject  # noqa: E402
from bacpypes3.local.device import DeviceObject  # noqa: E402
from bacpypes3.pdu import Address  # noqa: E402
from libbacnet._asyncio import BacnetClient  # noqa: E402

from tests.conftest import free_loopback_port  # noqa: E402

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_server(port: int, device_instance: int = 1001) -> NormalApplication:
    device = DeviceObject(
        objectIdentifier=f"device,{device_instance}",
        objectName=f"TestDevice{device_instance}",
        vendorIdentifier=999,
    )
    ai = AnalogInputObject(
        objectIdentifier="analogInput,0",
        objectName="AI0",
        presentValue=42.5,
        units="degreesCelsius",
    )
    app = NormalApplication(device, Address(("127.0.0.1", port)))
    app.add_object(ai)
    return app


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
async def bacnet_server() -> tuple[NormalApplication, int]:
    """Spin up a bacpypes3 NormalApplication on a free loopback port.

    Yields:
        (app, port) tuple.
    """
    port = free_loopback_port()
    app = _make_server(port, device_instance=5001)
    await asyncio.sleep(0.05)  # allow socket to bind
    return app, port


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


@pytest.mark.anyio
async def test_who_is_discovers_simulator() -> None:
    port = free_loopback_port()
    server = _make_server(port, device_instance=2001)
    async with BacnetClient(local_addr=("127.0.0.1", free_loopback_port())) as client:
        await asyncio.sleep(0.05)

        devices = await client.who_is(
            addr=libbacnet.BacnetAddr("127.0.0.1", port),
            wait=1.0,
        )
        assert len(devices) >= 1
        instances = [ev.message.device_id.instance for ev in devices]
        assert 2001 in instances
    server.close()


@pytest.mark.anyio
async def test_read_property_present_value() -> None:
    port = free_loopback_port()
    server = _make_server(port, device_instance=3001)
    async with BacnetClient(local_addr=("127.0.0.1", free_loopback_port())) as client:
        await asyncio.sleep(0.05)

        dest = libbacnet.BacnetAddr("127.0.0.1", port)
        obj_id = libbacnet.ObjectIdentifier(object_type=0, instance=0)

        payload = await client.read_property(addr=dest, obj_id=obj_id, prop_id=85)
        assert isinstance(payload, libbacnet.ReadPropertyResult)
        assert isinstance(payload.value, libbacnet.PropertyValueReal)
    server.close()


@pytest.mark.anyio
async def test_read_property_object_name() -> None:
    port = free_loopback_port()
    server = _make_server(port, device_instance=4001)
    async with BacnetClient(local_addr=("127.0.0.1", free_loopback_port())) as client:
        await asyncio.sleep(0.05)

        dest = libbacnet.BacnetAddr("127.0.0.1", port)
        obj_id = libbacnet.ObjectIdentifier(object_type=0, instance=0)

        payload = await client.read_property(addr=dest, obj_id=obj_id, prop_id=77)
        assert isinstance(payload, libbacnet.ReadPropertyResult)
        assert isinstance(payload.value, libbacnet.PropertyValueCharacterString)
    server.close()


@pytest.mark.anyio
async def test_read_property_timeout_on_unreachable_host() -> None:
    config = libbacnet.StackConfig(
        apdu_timeout_secs=0.2,
        apdu_retries=1,
        max_apdu_length=1476,
        max_segment_buffer=2 * 1024 * 1024,
    )
    async with BacnetClient(config=config, local_addr=("127.0.0.1", free_loopback_port())) as client:
        dest = libbacnet.BacnetAddr("127.0.0.1", 1)
        obj_id = libbacnet.ObjectIdentifier(object_type=0, instance=0)

        with pytest.raises(libbacnet.BacnetTimeoutError):
            await client.read_property(addr=dest, obj_id=obj_id, prop_id=85)
