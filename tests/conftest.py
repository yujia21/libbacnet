"""Shared pytest fixtures for libbacnet tests."""

from __future__ import annotations

import socket
import struct
from typing import TYPE_CHECKING
from unittest.mock import MagicMock

import pytest

libbacnet = pytest.importorskip("libbacnet", reason="libbacnet native extension not built")

from libbacnet._asyncio import BacnetProtocol  # noqa: E402

if TYPE_CHECKING:
    from collections.abc import Generator


# ---------------------------------------------------------------------------
# BACnet datagram builders (shared across test modules)
# ---------------------------------------------------------------------------


def make_simple_ack(invoke_id: int, service_choice: int = 12) -> bytes:
    """Build a minimal BVLC+NPDU+SimpleACK datagram.

    Args:
        invoke_id: The invoke ID to echo back.
        service_choice: The BACnet service choice byte (default 12 = ReadProperty).

    Returns:
        Raw bytes of a well-formed BACnet/IP SimpleACK datagram.
    """
    apdu = bytes([0x20, invoke_id, service_choice])
    npdu = bytes([0x01, 0x04]) + apdu
    bvlc_len = 4 + len(npdu)
    return bytes([0x81, 0x0A]) + struct.pack(">H", bvlc_len) + npdu


def make_i_am(device_instance: int = 42) -> bytes:
    """Build a minimal BVLC+NPDU+I-Am unconfirmed datagram.

    Args:
        device_instance: The device instance number to encode.

    Returns:
        Raw bytes of a well-formed BACnet/IP I-Am datagram.
    """

    def _app_uint(value: int, tag: int) -> bytes:
        if value < 256:
            return bytes([tag << 4 | 1, value])
        if value < 65536:
            return bytes([tag << 4 | 2]) + struct.pack(">H", value)
        return bytes([tag << 4 | 4]) + struct.pack(">I", value)

    oid_value = (8 << 22) | device_instance
    oid_bytes = bytes([0xC4]) + struct.pack(">I", oid_value)
    max_apdu_bytes = _app_uint(1476, 2)
    seg_bytes = _app_uint(0, 9)
    vendor_id_bytes = _app_uint(999, 2)

    apdu = bytes([0x10, 0x00]) + oid_bytes + max_apdu_bytes + seg_bytes + vendor_id_bytes
    npdu = bytes([0x01, 0x00]) + apdu
    bvlc_len = 4 + len(npdu)
    return bytes([0x81, 0x0A]) + struct.pack(">H", bvlc_len) + npdu


def free_loopback_port() -> int:
    """Return a free UDP port on 127.0.0.1.

    Returns:
        An available port number.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]  # type: ignore[no-any-return]


def make_complex_ack_read_property(invoke_id: int, service_data: bytes) -> bytes:
    """Build a BVLC+NPDU+ComplexACK datagram for ReadProperty (service choice 12).

    Args:
        invoke_id: The invoke ID to echo back.
        service_data: Encoded ReadProperty response service data bytes.

    Returns:
        Raw bytes of a well-formed BACnet/IP ComplexACK datagram.
    """
    apdu = bytes([0x30, invoke_id, 12]) + service_data
    npdu = bytes([0x01, 0x04]) + apdu
    bvlc_len = 4 + len(npdu)
    return bytes([0x81, 0x0A]) + struct.pack(">H", bvlc_len) + npdu


def make_complex_ack_read_property_multiple(invoke_id: int, service_data: bytes) -> bytes:
    """Build a BVLC+NPDU+ComplexACK datagram for ReadPropertyMultiple (service choice 14).

    Args:
        invoke_id: The invoke ID to echo back.
        service_data: Encoded ReadPropertyMultiple response service data bytes.

    Returns:
        Raw bytes of a well-formed BACnet/IP ComplexACK datagram.
    """
    apdu = bytes([0x30, invoke_id, 14]) + service_data
    npdu = bytes([0x01, 0x04]) + apdu
    bvlc_len = 4 + len(npdu)
    return bytes([0x81, 0x0A]) + struct.pack(">H", bvlc_len) + npdu


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def protocol_with_transport() -> Generator[tuple[BacnetProtocol, MagicMock], None, None]:
    """Provide a BacnetProtocol connected to a MagicMock transport.

    Yields:
        A (protocol, mock_transport) tuple ready for use in tests.
    """
    protocol = BacnetProtocol()
    transport = MagicMock()
    protocol.connection_made(transport)
    yield protocol, transport
    protocol.connection_lost(None)
