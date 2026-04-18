# libbacnet

A sans-IO BACnet/IP client library written in Rust with a Python asyncio interface.

## Overview

`libbacnet` implements the BACnet/IP client protocol as a pure state machine (the
[sans-IO](https://sans-io.readthedocs.io/) pattern).  All protocol logic lives in
a Rust `Stack` type that:

- receives inputs (`Received`, `Tick`, `Send`),
- returns outputs (`Transmit`, `Event`, `Deadline`),
- never touches a socket, spawns a thread, or reads a clock.

A thin Python asyncio layer (`BacnetProtocol` / `BacnetClient`) wires the stack
to UDP I/O, drives the retry scheduler, and exposes `async def` methods for each
supported service.

### Supported services

| Service | Direction |
|---|---|
| ReadProperty | confirmed request |
| ReadPropertyMultiple | confirmed request |
| WriteProperty | confirmed request |
| Who-Is / I-Am | unconfirmed |
| WhoIsRouterToNetwork / IAmRouterToNetwork | unconfirmed (NPDU) |

Segmented responses (ComplexACK with `more-follows`) are transparently
reassembled; large segmented requests are automatically fragmented.

### Non-goals (v0.1)

- MS/TP or other physical layers (BACnet/IP only)
- Server / device role
- COV subscriptions, alarms, trend logs
- BBMD / foreign device registration
- IPv6

---

## Quick start

```python
import asyncio
from libbacnet import BacnetClient, BacnetAddr, ObjectIdentifier, PropertyIdentifier

async def main():
    async with BacnetClient(local_addr=("0.0.0.0", 47808)) as client:
        # Discover devices (3-second collection window)
        devices = await client.who_is(wait=3.0)
        print(f"Found {len(devices)} device(s)")

        for ev in devices:
            addr = BacnetAddr(ev.src.addr, ev.src.port)
            obj = ObjectIdentifier(object_type=8, instance=ev.message.device_id.instance)

            # Read the device object's description — returns a ReadPropertyResult
            result = await client.read_property(
                addr=addr,
                obj_id=obj,
                prop_id=PropertyIdentifier.DESCRIPTION,
            )
            print(f"  {addr} description: {result.value}")

asyncio.run(main())
```

See [`examples/discover_read_write.py`](examples/discover_read_write.py) for a
longer example that performs Who-Is discovery, reads the object list, reads and
writes `present-value` on each object, and then reads all values with
`read_property_multiple`.

---

## Limitations and known constraints

- **BACnet/IP only** — MS/TP and other data links are not supported.
- **Client role only** — incoming confirmed requests from other devices are
  silently ignored.
- **IPv4 only** — `BacnetAddr` stores an IPv4 address; IPv6 BVLC is not
  implemented.
- **No BBMD** — the library sends `Original-Unicast-NPDU` (0x0A) and
  `Original-Broadcast-NPDU` (0x0B) only.  Foreign device registration and
  broadcast distribution are out of scope.
- **Single-threaded** — `Stack` is not `Send + Sync`; use it from a single
  asyncio event loop thread.
