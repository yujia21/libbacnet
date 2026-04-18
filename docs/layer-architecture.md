# Layer Architecture

This document maps the full module and layer hierarchy of `libbacnet`, from
raw wire bytes up to the Python `async def` API.

## Module map

```mermaid
graph TD
    subgraph Python["Python package  (python/libbacnet/)"]
        CLIENT["BacnetClient\nasync context manager\nread_property / write_property\nwho_is / who_is_router_to_network"]
        PROTO["BacnetProtocol\nasyncio.DatagramProtocol\nUDP I/O · timer · event queue"]
        ENUMS["_enums.py\nPropertyIdentifier · ObjectType\nErrorClass · ErrorCode\n(Python IntEnum)"]
    end

    subgraph Bindings["PyO3 bindings  (src/pyo3_bindings/)"]
        PYO3["pyo3_bindings/mod.rs\nExposes Rust types as Python classes\nInput* · Output* · Event* · Service*\nPropertyValue* · Result* · Stack · StackConfig"]
    end

    subgraph Rust["Rust crate  (src/)"]
        STACK["stack/mod.rs\nStack — sans-IO state machine\nprocess(Input) → Vec&lt;Output&gt;"]

        subgraph StackInternals["stack internals"]
            ADDR["addr.rs\nBacnetAddr"]
            IID["invoke_id.rs\nInvokeIdPool"]
            SLOT["slot.rs\nInFlightSlot"]
            SEG["segmentation.rs\nSendSegState · RecvSegState"]
            TYPES["types.rs\nInput · Output · BacnetEvent\nBacnetService · StackConfig"]
        end

        subgraph Services["services/"]
            SVC_MOD["mod.rs\nshared tag helpers"]
            RP["read_property.rs\nencode_request · decode_response"]
            RPM["read_property_multiple.rs\nencode_request · decode_response"]
            WP["write_property.rs\nencode_request"]
            WI["who_is.rs\nencode_who_is\nencode_who_is_router_to_network"]
            IAM["i_am.rs\ndecode_i_am\ndecode_i_am_router_to_network"]
        end

        subgraph Codec["codec/"]
            BVLC["bvlc.rs\nencode · decode\nBvlcFrame"]
            NPDU["npdu.rs\nencode · decode\nNpduFrame"]
            APDU["apdu.rs\nencode_confirmed_request\ndecode_complex_ack\ndecode_simple_ack\nencode/decode_segment_ack\ndecode_error · decode_abort · decode_reject"]
            CTYPES["types.rs\nPropertyValue (15 variants)\nObjectIdentifier · Date · Time\nDecodeError"]
        end

        ENUMS_RS["enums.rs\nPropertyIdentifier · ObjectType\nErrorClass · ErrorCode\n(Rust enums)"]
    end

    %% Vertical dependencies
    CLIENT --> PROTO
    PROTO --> PYO3
    CLIENT --> PYO3
    ENUMS -.->|"mirrors"| ENUMS_RS

    PYO3 --> STACK
    PYO3 --> ENUMS_RS

    STACK --> StackInternals
    STACK --> Services
    Services --> Codec
    Services --> CTYPES
    Codec --> CTYPES
    STACK --> ENUMS_RS
```

## Layer responsibilities

| Layer | Location | Responsibility |
|---|---|---|
| **BacnetClient** | `python/libbacnet/_asyncio.py` | High-level `async def` API. Builds service objects, awaits futures, decodes results. |
| **BacnetProtocol** | `python/libbacnet/_asyncio.py` | Wires the Rust `Stack` to a real UDP socket and asyncio timer. Owns the event queue. |
| **PyO3 bindings** | `src/pyo3_bindings/mod.rs` | Exposes every Rust type as a Python class. Converts Python objects → Rust enums → Python objects on the way back. |
| **Stack** | `src/stack/` | Pure sans-IO state machine. No I/O. No clock. Drives all BACnet protocol logic: invoke IDs, retries, timeouts, segmentation. |
| **Services** | `src/services/` | Service-specific encode/decode: ReadProperty, ReadPropertyMultiple, WriteProperty, Who-Is, I-Am, IAmRouterToNetwork. |
| **Codec** | `src/codec/` | Wire-format encode/decode for the four BACnet/IP layers: BVLC, NPDU, APDU, and application-typed values. |
| **Enums** | `src/enums.rs` / `python/libbacnet/_enums.py` | Shared BACnet enumeration values (property IDs, object types, error codes). Python side mirrors Rust side as `IntEnum`. |

## Key design boundaries

- **The `Stack` never crosses the Rust/Python boundary directly.** All
  crossing happens in `pyo3_bindings`, which converts between Rust `Input`/
  `Output` enums and their Python counterparts.

- **`#[cfg(not(test))]` isolates PyO3.** During `cargo test`, the
  `pyo3_bindings` module is excluded entirely, so Rust unit tests compile
  without a Python interpreter.

- **The codec layer has no knowledge of the stack.** `codec/` only encodes
  and decodes bytes; it holds no state and makes no decisions. The stack
  calls into the codec directly; the Python layer never touches the codec.
