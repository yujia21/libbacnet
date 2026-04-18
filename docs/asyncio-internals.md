# Python asyncio Layer Internals

This document shows how `BacnetProtocol` and `BacnetClient` wire the Rust
`Stack` to real UDP I/O and the asyncio event loop.

## Component relationships

```mermaid
graph TD
    subgraph App["Application code"]
        APP["await client.read_property(...)"]
    end

    subgraph Client["BacnetClient"]
        RPMETHOD["read_property / write_property\nread_property_multiple"]
        WHOIS["who_is /\nwho_is_router_to_network"]
        SEND_CONF["_send_confirmed(addr, service)\ncreates Future\ncalls _send_input\nawaits Future"]
        SEND_WI["_send_who_is / _send_who_is_router\nbuilds raw BVLC bytes\ncalls transport.sendto directly"]
        PENDING["_pending dict\n(addr, port, invoke_id) → Future"]
        ON_EVENT["_on_event(event)\nroutes BacnetEvent\nto correct Future or collector"]
        COLLECTOR["_who_is_collector list\naccumulates I-Am events\nduring wait window"]
    end

    subgraph Protocol["BacnetProtocol"]
        SEND_INPUT["send_input(inp)\ncalls stack.process(inp)\ncalls _handle_outputs"]
        HANDLE_OUT["_handle_outputs(outputs)\ndispatches each Output"]
        DO_TICK["_do_tick()\ncreates InputTick(now=monotonic)\ncalls send_input"]
        DGM_RECV["datagram_received(data, addr)\ncreates InputReceived\ncalls send_input"]
        TRANSMIT["_transmit(out)\ntransport.sendto(bytes, (addr, port))"]
        EMIT["_emit_event(event)\nqueue.put_nowait(event)\ncalls _on_event callback"]
        RESCHEDULE["_reschedule_tick(deadline)\ncancel old handle\ncall_later(deadline - now, _do_tick)"]
        EVENTS_Q["events: asyncio.Queue\n(public — for advanced use)"]
    end

    subgraph Stack["Rust Stack (sans-IO)"]
        PROCESS["Stack::process(Input)\n→ Vec&lt;Output&gt;"]
    end

    subgraph IO["asyncio event loop"]
        UDP_SOCK(["UDP socket\n(asyncio.DatagramTransport)"])
        TIMER(["asyncio timer\n(loop.call_later handle)"])
    end

    %% Application → Client
    APP --> RPMETHOD
    RPMETHOD --> SEND_CONF
    SEND_CONF --> SEND_INPUT
    SEND_CONF --> PENDING

    %% Protocol → Stack
    SEND_INPUT --> PROCESS
    DGM_RECV --> SEND_INPUT
    DO_TICK --> SEND_INPUT

    %% Stack → Protocol outputs
    PROCESS -->|"OutputTransmit"| TRANSMIT
    PROCESS -->|"OutputEvent"| EMIT
    PROCESS -->|"OutputDeadline"| RESCHEDULE

    %% Protocol → I/O
    TRANSMIT --> UDP_SOCK
    RESCHEDULE --> TIMER

    %% I/O → Protocol
    UDP_SOCK -->|"datagram_received"| DGM_RECV
    TIMER -->|"fires"| DO_TICK

    %% Event routing
    EMIT --> EVENTS_Q
    EMIT --> ON_EVENT
    ON_EVENT --> PENDING
    ON_EVENT --> COLLECTOR

    WHOIS --> SEND_WI
    WHOIS --> COLLECTOR
    SEND_WI --> UDP_SOCK
```

## Confirmed request sequence

```mermaid
sequenceDiagram
    participant App
    participant Client as BacnetClient
    participant Proto as BacnetProtocol
    participant Stack as Rust Stack
    participant UDP

    App->>Client: await read_property(addr, obj_id, prop_id)
    Client->>Client: create Future, store in _pending
    Client->>Proto: send_input(InputSend{service, dest})
    Proto->>Stack: stack.process(InputSend)
    Stack-->>Proto: [OutputTransmit, OutputDeadline]
    Proto->>UDP: transport.sendto(frame)
    Proto->>Proto: reschedule_tick(deadline)
    Proto-->>Client: raw outputs (extract invoke_id from OutputTransmit)

    note over UDP: network round-trip

    UDP->>Proto: datagram_received(response_bytes, src)
    Proto->>Stack: stack.process(InputReceived)
    Stack-->>Proto: [OutputEvent(EventResponse{invoke_id, payload})]
    Proto->>Client: _on_event(EventResponse)
    Client->>Client: _pending[key].set_result(payload)
    Client->>Client: decode_read_property(payload)
    Client-->>App: ReadPropertyResult
```

## Timer-driven retry sequence

```mermaid
sequenceDiagram
    participant ELoop as asyncio event loop
    participant Proto as BacnetProtocol
    participant Stack as Rust Stack
    participant UDP
    participant Client as BacnetClient

    ELoop->>Proto: call_later fires → _do_tick()
    Proto->>Stack: stack.process(InputTick{now})
    Stack-->>Proto: [OutputTransmit (retry), OutputDeadline]
    Proto->>UDP: transport.sendto(retry frame)
    Proto->>Proto: reschedule_tick(next deadline)

    note over Stack: after apdu_retries exhausted...

    ELoop->>Proto: call_later fires → _do_tick()
    Proto->>Stack: stack.process(InputTick{now})
    Stack-->>Proto: [OutputEvent(EventTimeout), OutputDeadline]
    Proto->>Client: _on_event(EventTimeout)
    Client->>Client: future.set_exception(BacnetTimeoutError)
```

## Who-Is / unconfirmed flow

Who-Is bypasses the confirmed-request machinery entirely — it does not go
through the Rust `Stack` at all for sending. The Python layer builds the raw
BVLC/NPDU/APDU bytes directly and calls `transport.sendto`. Incoming I-Am
responses *do* go through the stack (via `datagram_received`) and arrive as
`EventUnconfirmedReceived` events.

```mermaid
sequenceDiagram
    participant App
    participant Client as BacnetClient
    participant Proto as BacnetProtocol
    participant Stack as Rust Stack
    participant UDP

    App->>Client: await who_is(wait=3.0)
    Client->>Client: _who_is_collector = []
    Client->>UDP: transport.sendto(raw Who-Is BVLC bytes)
    note over Client: asyncio.sleep(3.0) — collection window

    UDP->>Proto: datagram_received(i_am_bytes, src)
    Proto->>Stack: stack.process(InputReceived)
    Stack-->>Proto: [OutputEvent(EventUnconfirmedReceived{I-Am})]
    Proto->>Client: _on_event(EventUnconfirmedReceived)
    Client->>Client: _who_is_collector.append(event)

    note over Client: sleep ends
    Client-->>App: list of EventUnconfirmedReceived
```

## Invoke ID extraction

After calling `stack.process(InputSend(...))`, `BacnetClient` needs to know
which invoke ID was allocated so it can key the pending Future. Because the
`Stack` embeds the invoke ID inside the encoded APDU bytes rather than
returning it as a separate field, `_extract_invoke_id` parses it back out of
the first `OutputTransmit` frame:

```
BVLC header  (4 bytes, fixed)
NPDU         (2 bytes minimum; may be longer if routing headers present)
APDU byte 0  (PDU type + flags)
APDU byte 1  (max-segments | max-APDU)
APDU byte 2  ← invoke ID
```

The NPDU length is determined dynamically by checking the `DNET_PRESENT`
control bit (`0x20`) to detect routing overhead.
