#!/usr/bin/env python3
"""
Example: discover BACnet devices with Who-Is, then read object-list.

Usage::

    uv run examples/discover_read_write.py [--host 0.0.0.0] [--port 47808] [--timeout 3.0]

If the target remote device does not respond to broadcasts, you can specify the
remote host and port to send the Who-Is request to::

    uv run examples/discover_read_write.py [--host 0.0.0.0] [--port 47808] \
        --remote-host <IP_ADDRESS> --remote-port <PORT>

Requires the libbacnet extension to be built::

    maturin develop
"""

from __future__ import annotations

import argparse
import asyncio
import logging
import sys

import libbacnet
from libbacnet import BacnetAddr, BacnetClient, ObjectIdentifier, PropertyIdentifier

_logger = logging.getLogger(__name__)


async def main(
    local_host: str,
    local_port: int,
    remote_host: str | None,
    remote_port: int | None,
    wait: float,
) -> None:
    """
    Discover BACnet devices with Who-Is, then read object-list from each device.

    Args:
        local_host: Local IP address to bind the UDP socket.
        local_port: Local UDP port to bind (default BACnet/IP port is 47808).
        remote_host: Remote IP address to send the Who-Is request to.
        remote_port: Remote UDP port to send the Who-Is request to.
        wait: Who-Is collection window in seconds.

    """
    client = BacnetClient(local_addr=(local_host, local_port))
    async with client:
        _logger.info("Sending Who-Is broadcast (waiting %.1fs for responses)...", wait)
        if remote_host is not None and remote_port is not None:
            remote_addr = libbacnet.BacnetAddr(remote_host, remote_port)
        else:
            remote_addr = None
        devices = await client.who_is(wait=wait, addr=remote_addr)

        if not devices:
            _logger.info("No BACnet devices found.")
            return

        _logger.info("Found %d device(s):", len(devices))
        for ev in devices:
            dev_id = ev.message.device_id
            _logger.info(
                "  Device instance %d at %s:%d (vendor=%s)",
                dev_id.instance,
                ev.src.addr,
                ev.src.port,
                ev.message.vendor_id,
            )

        _logger.info("Reading object-list from each device...")
        for ev in devices:
            dest = BacnetAddr(ev.src.addr, ev.src.port)
            obj_id = ObjectIdentifier(object_type=8, instance=ev.message.device_id.instance)
            try:
                obj_list = await client.read_property(
                    addr=dest,
                    obj_id=obj_id,
                    prop_id=PropertyIdentifier.OBJECT_LIST,  # object-list
                )
                _logger.info(
                    "  %s:%d object-list result: %s",
                    dest.addr,
                    dest.port,
                    obj_list,
                )
                if len(obj_list.value.values) >= 3:
                    for obj_id in obj_list.value.values[2:]:
                        res = await client.read_property(
                            addr=dest,
                            obj_id=obj_id,
                            prop_id=PropertyIdentifier.PRESENT_VALUE,
                        )  # presentValue
                        _logger.info("read_property: %s, %s", obj_id, res)
                        # change its value by +1
                        await client.write_property(
                            addr=dest,
                            obj_id=obj_id,
                            prop_id=PropertyIdentifier.PRESENT_VALUE,
                            value=libbacnet.PropertyValueReal(res.value.value + 1),
                        )
                        _logger.info("write_property: %s, %s", obj_id, res.value.value + 1)
                    # Now read all values again
                    request_list = [
                        libbacnet.ReadAccessSpec(
                            object_id=obj_id,
                            properties=[(PropertyIdentifier.PRESENT_VALUE, None)],
                        )
                        for obj_id in obj_list.value.values[2:]
                    ]
                    res = await client.read_property_multiple(addr=dest, request_list=request_list)
                    _logger.info("read_property_multiple: %s", res)
                    for obj in res.objects:
                        _logger.info("%s: %s", obj.object_id, obj.properties[0].value)

            except libbacnet.BacnetTimeoutError:
                _logger.warning("  %s:%d — timeout", dest.addr, dest.port)
            except libbacnet.BacnetError:
                _logger.exception("  %s:%d — error", dest.addr, dest.port)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="BACnet Who-Is + ReadProperty example")
    parser.add_argument("--host", default="0.0.0.0", help="Local bind address")
    parser.add_argument("--port", type=int, default=47808, help="Local UDP port")
    parser.add_argument("--timeout", type=float, default=3.0, help="Who-Is collection window (s)")
    parser.add_argument(
        "--remote-host",
        default=None,
        help="Remote IP address to send the Who-Is request to",
    )
    parser.add_argument(
        "--remote-port",
        type=int,
        default=None,
        help="Remote UDP port to send the Who-Is request to",
    )
    return parser.parse_args()


if __name__ == "__main__":
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )
    args = _parse_args()
    asyncio.run(main(args.host, args.port, args.remote_host, args.remote_port, args.timeout))
    sys.exit(0)
