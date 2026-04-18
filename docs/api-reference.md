
# API reference

## `BacnetClient`

High-level asyncio client.  Most users should start here.

```python
async with BacnetClient(config=None, local_addr=("0.0.0.0", 47808)) as client:
    # Confirmed services — return typed result objects
    result   = await client.read_property(addr, obj_id, prop_id, array_index=None)
    result   = await client.read_property_multiple(addr, request_list)
    await client.write_property(addr, obj_id, prop_id, value, array_index=None, priority=None)

    # Discovery — return lists of EventUnconfirmedReceived
    devices  = await client.who_is(addr=None, low=None, high=None, wait=3.0)
    routers  = await client.who_is_router_to_network(network=None, wait=3.0)
```

### Return types

| Method | Return type |
|---|---|
| `read_property` | `ReadPropertyResult` |
| `read_property_multiple` | `ReadPropertyMultipleResult` |
| `write_property` | `None` (raises on error) |
| `who_is` | `list[EventUnconfirmedReceived]` |
| `who_is_router_to_network` | `list[EventUnconfirmedReceived]` |

**`ReadPropertyResult`** — fields: `object_id`, `property_id`, `array_index`, `value` (`PropertyValue`).

**`ReadPropertyMultipleResult`** — field: `objects` (`list[ObjectResult]`).  Each
`ObjectResult` has `object_id` and `properties` (`list[PropertyResult]`).  Each
`PropertyResult` has either `value` (`PropertyValue`) or `error`
(`BacnetPropertyError`).

**`PropertyValue`** — one of: `PropertyValueNull`, `PropertyValueBoolean`,
`PropertyValueUnsigned`, `PropertyValueSigned`, `PropertyValueReal`,
`PropertyValueDouble`, `PropertyValueOctetString`, `PropertyValueCharacterString`,
`PropertyValueBitString`, `PropertyValueEnumerated`, `PropertyValueDate`,
`PropertyValueTime`, `PropertyValueObjectIdentifier`, `PropertyValueArray`,
`PropertyValueAny`.

## `BacnetProtocol`

Low-level `asyncio.DatagramProtocol`.  Use this when you need direct access to
the event stream or want to integrate the stack into an existing event loop.

```python
from libbacnet import BacnetProtocol

protocol = BacnetProtocol(config=None, on_event=None)

# Inject inputs manually (e.g. in tests)
outputs = protocol.send_input(InputSend(service=svc, dest=addr))

# Consume events from the async queue
event = await protocol.events.get()
```

## `StackConfig`

```python
from libbacnet import StackConfig

cfg = StackConfig(
    apdu_timeout_secs=3.0,   # seconds before retransmit
    apdu_retries=3,           # retransmit attempts before Timeout
    max_apdu_length=1476,     # outgoing segmentation threshold (bytes)
    max_segment_buffer=2_097_152,  # reassembly buffer limit (bytes)
)
```

## Enumerations

`PropertyIdentifier`, `ObjectType`, `ErrorClass`, and `ErrorCode` are Python
`IntEnum` classes exported directly from `libbacnet`.  They support unknown
values via a `_missing_` fallback that returns a dynamic `UNKNOWN_<value>`
member, so code never raises on an unrecognised wire value.

```python
from libbacnet import PropertyIdentifier, ObjectType

prop = PropertyIdentifier.PRESENT_VALUE   # 85
obj  = ObjectType.ANALOG_INPUT            # 0

# Pass enum members directly to client methods
result = await client.read_property(addr, obj_id, prop_id=PropertyIdentifier.OBJECT_LIST)
```

## `ReadAccessSpec`

Used to build `read_property_multiple` requests.

```python
from libbacnet import ReadAccessSpec, PropertyIdentifier

spec = ReadAccessSpec(
    object_id=obj_id,
    properties=[
        (PropertyIdentifier.PRESENT_VALUE, None),   # (prop_id, array_index)
        (PropertyIdentifier.DESCRIPTION, None),
    ],
)
result = await client.read_property_multiple(addr=dest, request_list=[spec])
```

## Exceptions

| Exception | Raised when |
|---|---|
| `BacnetTimeoutError` | No response within the retry budget |
| `BacnetError` | Server sent Error, Abort, or Reject PDU |
| `InvokeIdExhaustedError` | All 256 invoke IDs for the destination are in use |
