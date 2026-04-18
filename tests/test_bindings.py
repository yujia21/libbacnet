"""Python-level smoke tests for the PyO3 bindings (section 7.7).

These tests exercise the bindings using synthetic byte sequences that encode
known BACnet PDUs. They skip automatically if the native extension has not
been built yet (run ``maturin develop`` first).
"""

from __future__ import annotations

import struct

import pytest

libbacnet = pytest.importorskip("libbacnet", reason="libbacnet native extension not built")

from tests.conftest import make_i_am, make_simple_ack  # noqa: E402

# ---------------------------------------------------------------------------
# Module-level aliases for readability
# ---------------------------------------------------------------------------

Stack = libbacnet.Stack
StackConfig = libbacnet.StackConfig
BacnetAddr = libbacnet.BacnetAddr
InputTick = libbacnet.InputTick
InputReceived = libbacnet.InputReceived
InputSend = libbacnet.InputSend
OutputTransmit = libbacnet.OutputTransmit
OutputEvent = libbacnet.OutputEvent
OutputDeadline = libbacnet.OutputDeadline
EventResponse = libbacnet.EventResponse
EventTimeout = libbacnet.EventTimeout
EventUnconfirmedReceived = libbacnet.EventUnconfirmedReceived
UnconfirmedIAm = libbacnet.UnconfirmedIAm
ServiceReadProperty = libbacnet.ServiceReadProperty
ServiceWriteProperty = libbacnet.ServiceWriteProperty
ObjectIdentifier = libbacnet.ObjectIdentifier
PropertyValueUnsigned = libbacnet.PropertyValueUnsigned
PropertyValueReal = libbacnet.PropertyValueReal
PropertyValueCharacterString = libbacnet.PropertyValueCharacterString
PropertyValueNull = libbacnet.PropertyValueNull
PropertyValueBoolean = libbacnet.PropertyValueBoolean
InvokeIdExhaustedError = libbacnet.InvokeIdExhaustedError

DEST = BacnetAddr("192.168.1.1", 47808)
SRC = BacnetAddr("192.168.1.1", 47808)
NOW = 1_000.0


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def stack() -> Stack:
    """Return a Stack with default configuration."""
    return Stack()


@pytest.fixture
def read_property_service() -> ServiceReadProperty:
    """Return a ServiceReadProperty targeting Device:1 property 85."""
    oid = ObjectIdentifier(object_type=8, instance=1)
    return ServiceReadProperty(object_id=oid, property_id=85, array_index=None)


# ---------------------------------------------------------------------------
# Stack construction
# ---------------------------------------------------------------------------


def test_stack_default_config(stack: Stack) -> None:
    assert stack is not None


def test_stack_custom_config() -> None:
    cfg = StackConfig(apdu_timeout_secs=5.0, apdu_retries=2)
    s = Stack(config=cfg)
    assert s is not None


# ---------------------------------------------------------------------------
# InputTick
# ---------------------------------------------------------------------------


def test_tick_returns_deadline_when_slots_pending(
    stack: Stack,
    read_property_service: ServiceReadProperty,
) -> None:
    stack.process(InputSend(service=read_property_service, dest=DEST))
    tick_outputs = stack.process(InputTick(now=NOW))
    assert any(isinstance(o, OutputDeadline) for o in tick_outputs)


def test_tick_no_slots_returns_no_deadline(stack: Stack) -> None:
    outputs = stack.process(InputTick(now=NOW))
    assert not any(isinstance(o, OutputDeadline) for o in outputs)


# ---------------------------------------------------------------------------
# InputSend
# ---------------------------------------------------------------------------


def test_send_read_property_emits_transmit(stack: Stack) -> None:
    oid = ObjectIdentifier(object_type=8, instance=100)
    svc = ServiceReadProperty(object_id=oid, property_id=85, array_index=None)
    outputs = stack.process(InputSend(service=svc, dest=DEST))
    transmits = [o for o in outputs if isinstance(o, OutputTransmit)]
    assert len(transmits) == 1
    assert len(transmits[0].data) > 4


def test_send_write_property_emits_transmit(stack: Stack) -> None:
    oid = ObjectIdentifier(object_type=2, instance=1)
    svc = ServiceWriteProperty(
        object_id=oid,
        property_id=85,
        value=PropertyValueReal(value=23.5),
        array_index=None,
        priority=None,
    )
    outputs = stack.process(InputSend(service=svc, dest=DEST))
    assert any(isinstance(o, OutputTransmit) for o in outputs)


def test_send_also_returns_deadline(
    stack: Stack,
    read_property_service: ServiceReadProperty,
) -> None:
    outputs = stack.process(InputSend(service=read_property_service, dest=DEST))
    assert len([o for o in outputs if isinstance(o, OutputDeadline)]) == 1


# ---------------------------------------------------------------------------
# InputReceived
# ---------------------------------------------------------------------------


def test_simple_ack_emits_response_event(
    stack: Stack,
    read_property_service: ServiceReadProperty,
) -> None:
    send_outputs = stack.process(InputSend(service=read_property_service, dest=DEST))
    transmit = next(o for o in send_outputs if isinstance(o, OutputTransmit))
    invoke_id = transmit.data[7]

    ack = make_simple_ack(invoke_id, service_choice=12)
    recv_outputs = stack.process(InputReceived(data=list(ack), src=SRC))
    events = [o.event for o in recv_outputs if isinstance(o, OutputEvent)]
    assert len(events) == 1
    assert isinstance(events[0], EventResponse)
    assert events[0].invoke_id == invoke_id


def test_i_am_emits_unconfirmed_event(stack: Stack) -> None:
    i_am = make_i_am(device_instance=42)
    outputs = stack.process(InputReceived(data=list(i_am), src=SRC))
    events = [o.event for o in outputs if isinstance(o, OutputEvent)]
    assert len(events) == 1
    assert isinstance(events[0], EventUnconfirmedReceived)
    msg = events[0].message
    assert isinstance(msg, UnconfirmedIAm)
    assert msg.device_id.instance == 42


# ---------------------------------------------------------------------------
# Timeout
# ---------------------------------------------------------------------------


def test_tick_exhausts_retries_emits_timeout() -> None:
    cfg = StackConfig(apdu_timeout_secs=1.0, apdu_retries=1)
    s = Stack(config=cfg)
    oid = ObjectIdentifier(object_type=8, instance=1)
    svc = ServiceReadProperty(object_id=oid, property_id=85, array_index=None)
    s.process(InputSend(service=svc, dest=DEST))
    s.process(InputTick(now=1001.0))
    late_outputs = s.process(InputTick(now=3000.0))
    timeout_events = [o.event for o in late_outputs if isinstance(o, OutputEvent) and isinstance(o.event, EventTimeout)]
    assert len(timeout_events) == 1


# ---------------------------------------------------------------------------
# InvokeIdExhaustedError
# ---------------------------------------------------------------------------


def test_256_requests_exhaust_pool(
    stack: Stack,
    read_property_service: ServiceReadProperty,
) -> None:
    for _ in range(256):
        stack.process(InputSend(service=read_property_service, dest=DEST))
    with pytest.raises(InvokeIdExhaustedError):
        stack.process(InputSend(service=read_property_service, dest=DEST))


# ---------------------------------------------------------------------------
# PropertyValue types
# ---------------------------------------------------------------------------


def test_property_value_null() -> None:
    pv = PropertyValueNull()
    assert repr(pv) == "PropertyValueNull()"


def test_property_value_boolean() -> None:
    pv = PropertyValueBoolean(value=True)
    assert pv.value is True


def test_property_value_unsigned() -> None:
    pv = PropertyValueUnsigned(value=42)
    assert pv.value == 42


def test_property_value_real() -> None:
    pv = PropertyValueReal(value=3.14)
    assert abs(pv.value - 3.14) < 0.001


def test_property_value_character_string() -> None:
    pv = PropertyValueCharacterString(value="hello")
    assert pv.value == "hello"


# ---------------------------------------------------------------------------
# decode_read_property / decode_read_property_multiple  (section 7.4c)
# ---------------------------------------------------------------------------

# Helpers to build ReadProperty and ReadPropertyMultiple service data bytes
# using the same encoding as the Rust unit tests in services/read_property.rs.


def _encode_context_oid(tag: int, oid_value: int) -> bytes:
    """Encode a context-tagged ObjectIdentifier (4-byte value)."""
    return bytes([(tag << 4) | 0x0C]) + oid_value.to_bytes(4, "big")


def _encode_context_unsigned_small(tag: int, value: int) -> bytes:
    """Encode a context-tagged unsigned int fitting in 1 byte."""
    return bytes([(tag << 4) | 0x09, value])


def _make_rp_service_data(object_type: int, instance: int, prop_id: int, pv_bytes: bytes) -> bytes:
    """Build ReadProperty ComplexACK service data for a single-value response."""
    oid_value = (object_type << 22) | instance
    data = _encode_context_oid(0, oid_value)
    data += _encode_context_unsigned_small(1, prop_id)
    data += bytes([0x3E])  # opening tag [3]
    data += pv_bytes
    data += bytes([0x3F])  # closing tag [3]
    return data


def _make_rpm_service_data_success(object_type: int, instance: int, prop_id: int, pv_bytes: bytes) -> bytes:
    """Build ReadPropertyMultiple ComplexACK service data (single object, single property, success)."""

    oid_value = (object_type << 22) | instance
    data = _encode_context_oid(0, oid_value)
    data += bytes([0x1E])  # opening tag [1]
    data += bytes([0x2E])  # opening tag [2]
    data += _encode_context_unsigned_small(0, prop_id)
    data += bytes([0x4E])  # opening tag [4] (success)
    data += pv_bytes
    data += bytes([0x4F])  # closing tag [4]
    data += bytes([0x2F])  # closing tag [2]
    data += bytes([0x1F])  # closing tag [1]
    return data


def _make_rpm_service_data_error(
    object_type: int,
    instance: int,
    prop_id: int,
    error_class: int,
    error_code: int,
) -> bytes:
    """Build ReadPropertyMultiple service data with a per-property error."""
    oid_value = (object_type << 22) | instance
    data = _encode_context_oid(0, oid_value)
    data += bytes([0x1E])  # opening tag [1]
    data += bytes([0x2E])  # opening tag [2]
    data += _encode_context_unsigned_small(0, prop_id)
    data += bytes([0x5E])  # opening tag [5] (error)
    data += bytes([0x91, error_class])  # enumerated error_class
    data += bytes([0x91, error_code])  # enumerated error_code
    data += bytes([0x5F])  # closing tag [5]
    data += bytes([0x2F])  # closing tag [2]
    data += bytes([0x1F])  # closing tag [1]
    return data


def test_decode_read_property_returns_typed_result() -> None:
    """5.1 — decode_read_property returns ReadPropertyResult with correct fields."""
    # Device:5, prop 85, value Real(23.5)
    pv_bytes = bytes([0x44]) + struct.pack(">f", 23.5)
    svc_data = _make_rp_service_data(8, 5, 85, pv_bytes)

    result = libbacnet.decode_read_property(list(svc_data))

    assert isinstance(result, libbacnet.ReadPropertyResult)
    assert result.object_id.object_type == 8
    assert result.object_id.instance == 5
    assert result.property_id == 85
    assert result.array_index is None
    assert isinstance(result.value, libbacnet.PropertyValueReal)
    assert abs(result.value.value - 23.5) < 0.001


def test_decode_read_property_multiple_returns_typed_result() -> None:
    """5.2 — decode_read_property_multiple returns ReadPropertyMultipleResult with nested objects."""
    pv_bytes = bytes([0x44]) + struct.pack(">f", 1.0)
    svc_data = _make_rpm_service_data_success(8, 1, 85, pv_bytes)

    result = libbacnet.decode_read_property_multiple(list(svc_data))

    assert isinstance(result, libbacnet.ReadPropertyMultipleResult)
    assert len(result.objects) == 1
    obj = result.objects[0]
    assert isinstance(obj, libbacnet.ObjectResult)
    assert obj.object_id.object_type == 8
    assert obj.object_id.instance == 1
    assert len(obj.properties) == 1
    prop = obj.properties[0]
    assert isinstance(prop, libbacnet.PropertyResult)
    assert prop.property_id == 85
    assert prop.array_index is None
    assert isinstance(prop.value, libbacnet.PropertyValueReal)
    assert abs(prop.value.value - 1.0) < 0.001


def test_decode_read_property_multiple_per_property_error_is_bacnet_property_error() -> None:
    """5.3 — per-property RPM error is a BacnetPropertyError value, not an exception."""
    svc_data = _make_rpm_service_data_error(8, 1, 85, error_class=2, error_code=31)

    result = libbacnet.decode_read_property_multiple(list(svc_data))

    assert len(result.objects) == 1
    prop = result.objects[0].properties[0]
    assert isinstance(prop.value, libbacnet.BacnetPropertyError)
    assert prop.value.error_class == 2
    assert prop.value.error_code == 31
    # Must NOT be an exception
    assert not isinstance(prop.value, Exception)


def test_bacnet_property_error_attributes() -> None:
    """5.3b — BacnetPropertyError has readable error_class and error_code."""
    err = libbacnet.BacnetPropertyError(error_class=2, error_code=31)
    assert err.error_class == 2
    assert err.error_code == 31


def test_new_types_importable() -> None:
    """5.5 — New result types are importable from libbacnet."""
    from libbacnet import (  # noqa: PLC0415
        BacnetPropertyError,
        ObjectResult,
        PropertyResult,
        ReadPropertyMultipleResult,
        ReadPropertyResult,
    )

    assert ReadPropertyResult is not None
    assert ReadPropertyMultipleResult is not None
    assert ObjectResult is not None
    assert PropertyResult is not None
    assert BacnetPropertyError is not None


# ---------------------------------------------------------------------------
# Section 7 — BACnet enumeration types (tasks 7.5–7.16)
# ---------------------------------------------------------------------------


def test_property_identifier_int_compat():
    """7.5 — PropertyIdentifier.PRESENT_VALUE == 85 and is int."""
    from libbacnet import PropertyIdentifier  # noqa: PLC0415

    assert PropertyIdentifier.PRESENT_VALUE == 85
    assert isinstance(PropertyIdentifier.PRESENT_VALUE, int)


def test_property_identifier_unknown():
    """7.6 — PropertyIdentifier(9999) returns pseudo-member named UNKNOWN_9999."""
    from libbacnet import PropertyIdentifier  # noqa: PLC0415

    v = PropertyIdentifier(9999)
    assert v == 9999
    assert v.name == "UNKNOWN_9999"


def test_object_type_int_compat():
    """7.7 — ObjectType.DEVICE == 8 and is int."""
    from libbacnet import ObjectType  # noqa: PLC0415

    assert ObjectType.DEVICE == 8
    assert isinstance(ObjectType.DEVICE, int)


def test_object_type_unknown():
    """7.8 — ObjectType(9999) returns pseudo-member without raising."""
    from libbacnet import ObjectType  # noqa: PLC0415

    v = ObjectType(9999)
    assert v == 9999


def test_error_class_int_compat():
    """7.9 — ErrorClass.PROPERTY == 2 and is int."""
    from libbacnet import ErrorClass  # noqa: PLC0415

    assert ErrorClass.PROPERTY == 2
    assert isinstance(ErrorClass.PROPERTY, int)


def test_error_code_unknown():
    """7.10 — ErrorCode(9999) returns pseudo-member without raising."""
    from libbacnet import ErrorCode  # noqa: PLC0415

    v = ErrorCode(9999)
    assert v == 9999


def test_decode_read_property_returns_property_identifier():
    """7.11 + 7.12 — decode_read_property result has PropertyIdentifier property_id."""
    from libbacnet import PropertyIdentifier, decode_read_property  # noqa: PLC0415

    data = _make_rp_service_data(8, 5, 85, bytes([0x44]) + struct.pack(">f", 1.0))
    result = decode_read_property(data)
    assert isinstance(result.property_id, PropertyIdentifier)
    assert result.property_id == 85
    assert result.property_id == PropertyIdentifier.PRESENT_VALUE


def test_decode_rpm_property_result_has_property_identifier():
    """7.13 — PropertyResult.property_id is a PropertyIdentifier instance."""
    from libbacnet import PropertyIdentifier, decode_read_property_multiple  # noqa: PLC0415

    data = _make_rpm_service_data_success(8, 1, 85, bytes([0x44]) + struct.pack(">f", 1.0))
    result = decode_read_property_multiple(data)
    prop = result.objects[0].properties[0]
    assert isinstance(prop.property_id, PropertyIdentifier)
    assert prop.property_id == PropertyIdentifier.PRESENT_VALUE


def test_decoded_object_identifier_object_type_is_enum():
    """7.14 — ObjectIdentifier.object_type returned by decode is an ObjectType instance."""
    from libbacnet import ObjectType, decode_read_property  # noqa: PLC0415

    data = _make_rp_service_data(8, 5, 85, bytes([0x44]) + struct.pack(">f", 1.0))
    result = decode_read_property(data)
    assert isinstance(result.object_id.object_type, ObjectType)
    assert result.object_id.object_type == 8
    assert result.object_id.object_type == ObjectType.DEVICE


def test_decoded_bacnet_property_error_uses_enum_types():
    """7.15 — BacnetPropertyError.error_class is ErrorClass; .error_code is ErrorCode."""
    from libbacnet import ErrorClass, ErrorCode, decode_read_property_multiple  # noqa: PLC0415

    data = _make_rpm_service_data_error(8, 1, 77, error_class=2, error_code=31)
    result = decode_read_property_multiple(data)
    err = result.objects[0].properties[0].value
    assert isinstance(err.error_class, ErrorClass)
    assert isinstance(err.error_code, ErrorCode)
    assert err.error_class == ErrorClass.PROPERTY
    assert err.error_code == 31  # READ_ACCESS_DENIED


def test_enum_imports_from_libbacnet():
    """7.16 — All four enum types importable from libbacnet top-level."""
    from libbacnet import ErrorClass, ErrorCode, ObjectType, PropertyIdentifier  # noqa: PLC0415

    assert PropertyIdentifier is not None
    assert ObjectType is not None
    assert ErrorClass is not None
    assert ErrorCode is not None
