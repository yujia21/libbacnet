# Encoding Pipeline

This document shows how a high-level service request (e.g.
`ServiceReadProperty`) is transformed into wire bytes through the four
BACnet/IP protocol layers, and how the reverse decoding path works.

---

## Encode path — request

```mermaid
flowchart TD
    A["BacnetService::ReadProperty\n{ object_id, property_id, array_index }"]

    A --> B["services/read_property.rs\nencode_request()\nctx[0] OID + ctx[1] prop_id\n+ ctx[2] array_index?"]

    B --> C["service_data: Vec&lt;u8&gt;\n(BACnet service-data bytes)"]

    C --> D["codec/apdu.rs\nencode_confirmed_request()\nConfirmedRequestParams {\n  invoke_id, service_choice=12,\n  segmentation=None,\n  service_data\n}"]

    D --> E["APDU bytes\n[ PDU_type|flags, max_seg|max_apdu,\n  invoke_id, service_choice,\n  ...service_data ]"]

    E --> F["codec/npdu.rs\nencode(NpduEncodeParams {\n  apdu,\n  data_expecting_reply=true,\n  priority=Normal,\n  dest=None, src=None\n})"]

    F --> G["NPDU bytes\n[ 0x01, ctrl_byte, ...apdu ]"]

    G --> H["codec/bvlc.rs\nencode(OriginalUnicastNpdu, npdu)"]

    H --> I["Final BVLC frame\n[ 0x81, 0x0A, len_hi, len_lo,\n  ...npdu ]"]

    I --> J["Output::Transmit { data, dest }\n→ transport.sendto()"]
```

### Byte layout — unsegmented ReadProperty request

```
Offset  Len  Field
──────  ───  ─────────────────────────────────────────────────────────
 0       1   BVLC type       0x81
 1       1   BVLC function   0x0A  (OriginalUnicastNpdu)
 2       2   BVLC length     big-endian total frame length
 4       1   NPDU version    0x01
 5       1   NPDU control    0x04  (data-expecting-reply, normal priority)
 6       1   APDU byte 0     0x00  (ConfirmedRequest, no seg flags)
 7       1   APDU byte 1     max-segments | max-APDU-length  (0x05 = up to 1476 B)
 8       1   invoke_id
 9       1   service_choice  0x0C  (ReadProperty = 12)
10       5   ctx[0] OID      0x0C + 4-byte big-endian (object_type<<22 | instance)
15       2   ctx[1] prop_id  0x19 + 1-byte prop_id  (or longer for large IDs)
17       *   ctx[2] arr_idx  optional — only present if array_index is Some
```

---

## Decode path — response

```mermaid
flowchart TD
    A["Raw UDP bytes\narriving from network"]

    A --> B["codec/bvlc.rs\ndecode()\nvalidate 0x81 header\ncheck length\nextract npdu slice"]

    B --> C["BvlcFrame { function, npdu }"]

    C --> D["codec/npdu.rs\ndecode()\nparse version + ctrl byte\noptional DNET/DADR/SNET/SADR\nextract apdu slice"]

    D --> E["NpduFrame { priority, is_network_layer_msg,\ndest, src, apdu }"]

    E --> F{"apdu[0] >> 4\n(PDU type)"}

    F -->|"0x03 ComplexACK"| G["codec/apdu.rs\ndecode_complex_ack()\nparse invoke_id, service_choice\nextract service_data"]

    F -->|"0x02 SimpleACK"| H["codec/apdu.rs\ndecode_simple_ack()\nparse invoke_id, service_choice"]

    F -->|"0x04 SegmentACK"| I["codec/apdu.rs\ndecode_segment_ack()"]

    F -->|"0x05 Error"| J["codec/apdu.rs\ndecode_error_apdu()\nerror_class + error_code"]

    F -->|"0x07 Abort"| K["codec/apdu.rs\ndecode_abort()\nabort_reason"]

    F -->|"0x01 UnconfirmedRequest"| L["service dispatch\non apdu[1] (service_choice)"]

    G --> M{"service_choice?"}
    M -->|"12 ReadProperty"| N["services/read_property.rs\ndecode_response(service_data)"]
    M -->|"14 ReadPropertyMultiple"| O["services/read_property_multiple.rs\ndecode_response(service_data)"]

    N --> P["codec/types.rs\nPropertyValue::decode()\nfor each application-tagged value"]
    O --> P

    P --> Q["ReadPropertyResult /\nReadPropertyMultipleResult"]

    L --> R{"apdu[1]?"}
    R -->|"0x00 I-Am"| S["services/i_am.rs\ndecode_i_am()"]
    R -->|others| T["(ignored — client role only)"]

    E -->|"is_network_layer_msg=true"| U["apdu[0] byte (network msg type)"]
    U -->|"0x01 IAmRouterToNetwork"| V["services/i_am.rs\ndecode_i_am_router_to_network()"]
```

---

## Service-data encoding in detail

### ReadProperty request (`service_choice = 12`)

```mermaid
flowchart LR
    A["object_id\nObjectIdentifier"] -->|"encode_context_object_id(tag=0)"| B["0x0C + 4 bytes\n(type&lt;&lt;22 | instance)"]
    C["property_id: u32"] -->|"encode_context_unsigned(tag=1)"| D["0x19 + N bytes\n(minimum-width)"]
    E["array_index: Option&lt;u32&gt;"] -->|"encode_context_unsigned(tag=2)\nonly if Some"| F["0x29 + N bytes"]
    B --> G["service_data"]
    D --> G
    F --> G
```

### ReadPropertyMultiple request (`service_choice = 14`)

```mermaid
flowchart LR
    subgraph spec["For each ReadAccessSpec"]
        A["object_id"] -->|"ctx[0] OID"| B
        B["opening tag [1]  (0x1E)"] --> C
        subgraph propref["For each property reference"]
            C["prop_id"] -->|"ctx[0] unsigned"| D["encoded prop bytes"]
            E["array_index?"] -->|"ctx[1] unsigned, only if Some"| D
        end
        D --> F["closing tag [1]  (0x1F)"]
    end
```

### WriteProperty request (`service_choice = 15`)

```mermaid
flowchart LR
    A["object_id"] -->|"ctx[0] OID"| G
    B["property_id"] -->|"ctx[1] unsigned"| G
    C["array_index?"] -->|"ctx[2] unsigned"| G
    D["opening tag [3]  (0x3E)"] --> G
    E["PropertyValue"] -->|"encode_tag_and_value()"| G
    F["closing tag [3]  (0x3F)"] --> G
    H["priority?"] -->|"ctx[4] unsigned"| G
    G["service_data"]
```

---

## PropertyValue encoding

`PropertyValue` uses BACnet **application tags** (upper nibble = tag number,
lower nibble = length or extended-length indicator). All values are encoded
and decoded in `codec/types.rs`.

```mermaid
flowchart TD
    A["PropertyValue variant"] --> B{"variant"}

    B -->|Null| C["0x00"]
    B -->|Boolean| D["0x11 + 0x00/0x01"]
    B -->|Unsigned| E["0x2N + big-endian bytes\n(minimum width: 1/2/3/4)"]
    B -->|Signed| F["0x3N + big-endian bytes"]
    B -->|Real| G["0x44 + 4 bytes IEEE 754"]
    B -->|Double| H["0x55 0x08 + 8 bytes IEEE 754"]
    B -->|OctetString| I["0x6N + raw bytes"]
    B -->|CharacterString| J["0x7N + encoding_byte + UTF-8 bytes"]
    B -->|BitString| K["0x8N + used_bits_byte + bit bytes"]
    B -->|Enumerated| L["0x9N + big-endian bytes"]
    B -->|Date| M["0xA4 + year-1900 + month + day + weekday"]
    B -->|Time| N["0xB4 + hour + minute + second + hundredths"]
    B -->|ObjectIdentifier| O["0xC4 + 4 bytes (type&lt;&lt;22|instance)"]
    B -->|Array| P["opening[3] + each element encoded + closing[3]"]
    B -->|Any| Q["raw bytes (pass-through)"]
```

Tag number `N` in the lower nibble encodes the byte length inline for values
of 1–4 bytes. Values of 5+ bytes use the extended-length form
(`0xN5 + length_byte + ...`).

---

## NPDU routing header (optional)

When a message is routed through a BACnet router, the NPDU carries optional
DNET/DADR (destination network/address) and SNET/SADR (source
network/address) fields. libbacnet generates plain unicast and broadcast
frames with no routing headers (`dest=None`, `src=None`).

```
ctrl byte bit layout:
  bit 7  NETWORK_LAYER_MSG  — set for network-layer messages (IAmRouter etc.)
  bit 5  DNET_PRESENT       — destination network address follows
  bit 3  SNET_PRESENT       — source network address follows
  bit 2  DATA_EXPECTING_REPLY — set for confirmed requests
  bit 1:0 PRIORITY          — 0=Normal, 1=Urgent, 2=CriticalEquipment, 3=LifeSafety
```
