
# Stack architecture

These diagrams represent the Rust Stack and its interactions with the Python asyncio layer.

```mermaid
flowchart TD
    subgraph Python["Python asyncio layer"]
        CLIENT["BacnetClient\n(high-level API)"]
        PROTO["BacnetProtocol\n(DatagramProtocol)"]
        UDP(["UDP socket"])
        TIMER(["asyncio timer"])
    end

    subgraph Rust["Rust Stack (sans-IO state machine)"]
        direction TB
        PROCESS["Stack::process(Input) → Vec&lt;Output&gt;"]

        subgraph Inputs["Inputs"]
            IN_RECV["Input::Received\n{ data, src }"]
            IN_TICK["Input::Tick\n{ now: f64 }"]
            IN_SEND["Input::Send\n{ service, dest }"]
        end

        subgraph Outputs["Outputs"]
            OUT_TX["Output::Transmit\n{ data, dest }"]
            OUT_EV["Output::Event\n(BacnetEvent)"]
            OUT_DL["Output::Deadline\n(f64 timestamp)"]
        end

        subgraph State["Internal state"]
            POOL["InvokeIdPool\nper-dest 256-bit bitset"]
            SLOTS["InFlightSlot map\n(dest, invoke_id) → slot"]
            SENDSEG["SendSegState map\nlarge outgoing requests"]
            RECVSEG["RecvSegState map\nreassembly buffers"]
        end

        PROCESS --> POOL
        PROCESS --> SLOTS
        PROCESS --> SENDSEG
        PROCESS --> RECVSEG
    end

    %% Host → Stack
    UDP -->|"datagram received"| IN_RECV
    TIMER -->|"timer fires"| IN_TICK
    CLIENT -->|"read/write/who-is"| IN_SEND

    IN_RECV --> PROCESS
    IN_TICK --> PROCESS
    IN_SEND --> PROCESS

    %% Stack → Host
    PROCESS --> OUT_TX
    PROCESS --> OUT_EV
    PROCESS --> OUT_DL

    OUT_TX -->|"send datagram"| UDP
    OUT_EV -->|"deliver result"| CLIENT
    OUT_DL -->|"re-arm timer"| TIMER

    PROTO -.->|"owns"| PROCESS
    CLIENT -.->|"wraps"| PROTO
```

### Input::Send — confirmed request path

```mermaid
flowchart TD
    A["Input::Send { service, dest }"] --> B["Allocate invoke_id\nfrom InvokeIdPool"]
    B --> C{"service_data size\n≤ max_apdu_length?"}

    C -->|yes — unsegmented| D["Encode APDU +\nwrap in NPDU + BVLC"]
    D --> E["Store InFlightSlot\n{ frame, retry_count=0,\nnext_retry_at = now+timeout }"]
    E --> F["Output::Transmit\nOutput::Deadline"]

    C -->|no — segmented| G["Create SendSegState\nslice into fragments"]
    G --> H["Send first window\n(up to 4 frames)"]
    H --> I["Store placeholder slot\n{ request_bytes=[], next_retry_at=∞ }"]
    I --> J["Output::Transmit ×N\nOutput::Deadline"]
```

### Input::Received — response dispatch

```mermaid
flowchart TD
    A["Input::Received { data, src }"] --> B["Decode BVLC → NPDU → APDU"]
    B --> C{"APDU PDU type"}

    C -->|"ComplexACK\nunsegmented"| D["free_slot(dest, invoke_id)"]
    D --> E["Output::Event\n(BacnetEvent::Response)"]

    C -->|"ComplexACK\nsegmented"| F["RecvSegState::\naccept_fragment()"]
    F --> G{"Reassembly\ncomplete?"}
    G -->|yes| D
    G -->|no — send SegmentACK| H["Output::Transmit\n(SegmentACK)"]

    C -->|SimpleACK| D
    C -->|SegmentACK| I["SendSegState::\nhandle_seg_ack()\nadvance window"]
    C -->|"Error / Abort\n/ Reject"| J["free_slot(dest, invoke_id)\nOutput::Event(Error/Abort)"]
    C -->|"UnconfirmedRequest\n(I-Am, IAmRouter…)"| K["Output::Event\n(UnconfirmedReceived)"]
```

### Input::Tick — retry and timeout sweep

```mermaid
flowchart TD
    A["Input::Tick { now }"] --> B["Pass 1: unsegmented retries\nfor each InFlightSlot\nwhere request_bytes ≠ empty"]
    B --> C{"now ≥ next_retry_at?"}
    C -->|no| D["skip"]
    C -->|yes| E{"retry_count\n≥ apdu_retries?"}
    E -->|no| F["retransmit frame\nincrement retry_count\nadvance next_retry_at"]
    E -->|yes| G["free_slot\nOutput::Event(Timeout)"]

    A --> H["Pass 2: send-seg window timeouts\nfor each SendSegState"]
    H --> I{"now ≥ window_sent_at\n+ SEG_TIMEOUT?"}
    I -->|yes| J["retransmit_window()"]
    I -->|no| K["skip"]

    A --> L["Pass 3: recv-seg reassembly timeouts\nfor each RecvSegState"]
    L --> M{"now ≥ last_activity\n+ SEG_TIMEOUT?"}
    M -->|yes| N["drop buffer\nOutput::Event(Abort reason=5)"]
    M -->|no| O["skip"]

    G --> P["Output::Deadline(earliest pending)"]
    J --> P
    N --> P
```
