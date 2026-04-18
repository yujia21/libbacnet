# Segmentation State Machines

BACnet segmentation splits large messages across multiple APDU frames when
they exceed `max_apdu_length`. libbacnet implements both sides:
`SendSegState` (outgoing large requests) and `RecvSegState` (incoming
segmented responses).

Constants used throughout:

| Constant | Value | Meaning |
|---|---|---|
| `DEFAULT_WINDOW_SIZE` | 4 | Segments sent per window before waiting for SegmentACK |
| `SEG_TIMEOUT_SECS` | 2.0 s | Window-level retransmit / reassembly timeout |
| `ABORT_REASON_REASSEMBLY_TIMEOUT` | 5 | BACnet abort reason byte for timeout |

---

## Send side — `SendSegState`

Used when an outgoing service payload exceeds `max_apdu_length`. The payload
is pre-sliced into fixed-size fragments; a *window* of frames is sent at once,
then the sender waits for a cumulative `SegmentACK` before advancing.

```mermaid
stateDiagram-v2
    [*] --> SendingWindow : SendSegState created, first window transmitted

    note right of SendingWindow
        Fields: window_start, window_size,
        window_sent_at, total_fragments,
        fragment_size, service_data
    end note

    SendingWindow --> SendingWindow : SegmentACK received, fragments remaining, advance window

    SendingWindow --> Complete : SegmentACK received, all fragments acknowledged

    SendingWindow --> SendingWindow : window timed out, retransmit_window called
```

### Window flow

```mermaid
sequenceDiagram
    participant Sender as libbacnet (SendSegState)
    participant Peer as Remote device

    Sender->>Peer: Seg 0 (more-follows=true)
    Sender->>Peer: Seg 1 (more-follows=true)
    Sender->>Peer: Seg 2 (more-follows=true)
    Sender->>Peer: Seg 3 (more-follows=true, window full)

    Peer-->>Sender: SegmentACK(seq=3, window_size=4)
    note over Sender: advance window_start to 4

    Sender->>Peer: Seg 4 (more-follows=true)
    Sender->>Peer: Seg 5 (more-follows=false ← last fragment)

    Peer-->>Sender: SegmentACK(seq=5)
    note over Sender: new_window_start(6) ≥ total_fragments(6) → Complete

    note over Sender,Peer: If SegmentACK not received within SEG_TIMEOUT_SECS...
    Sender->>Peer: Seg 4 (retransmit)
    Sender->>Peer: Seg 5 (retransmit)
```

---

## Receive side — `RecvSegState`

Used when an incoming ComplexACK arrives with the `more-follows` flag set.
Fragments are accumulated in a sparse `Vec<Option<Vec<u8>>>` indexed by
sequence number; a `SegmentACK` is sent back after each window.

```mermaid
stateDiagram-v2
    [*] --> Reassembling : RecvSegState created, first fragment received

    note right of Reassembling
        Fields: fragments, total_fragments,
        window_size, window_start,
        last_activity, bytes_accumulated,
        max_buffer
    end note

    Reassembling --> Reassembling : fragment received with more_follows true, send SegmentACK at window end

    Reassembling --> Complete : final fragment received, all slots filled, payload reassembled

    Reassembling --> TimedOut : last_activity timeout exceeded

    note right of TimedOut
        EventAbort reason 5 emitted
        state removed from recv_seg map
    end note
```

### `RecvAction` values

```mermaid
flowchart TD
    A["accept_fragment(seq, more_follows, proposed_window_size, data, ...)"] --> B{"bytes_accumulated\n> max_buffer?"}
    B -->|yes| C["RecvAction::Abort(5)\nbuffer overflow guard"]

    B -->|no| D["Store fragment\nupdate last_activity\nupdate window_size"]
    D --> E{"is_window_complete?\nseq ≥ window_start + window_size - 1\nOR more_follows == false"}

    E -->|no| F["RecvAction::Continue\n(no SegmentACK yet)"]

    E -->|yes| G["encode SegmentACK\nadvance window_start = seq + 1"]
    G --> H{"more_follows == false\nAND all fragments received?"}

    H -->|no — more windows expected| I["RecvAction::SendSegAck(frame)"]
    H -->|yes — reassembly complete| J["Concatenate all fragment payloads\nRecvAction::Complete { seg_ack_frame, payload }"]
```

### Reassembly flow

```mermaid
sequenceDiagram
    participant Peer as Remote device
    participant Recv as libbacnet (RecvSegState)

    Peer->>Recv: ComplexACK Seg 0 (more-follows=true)
    note over Recv: Continue (window not yet full)
    Peer->>Recv: ComplexACK Seg 1 (more-follows=true)
    note over Recv: Continue
    Peer->>Recv: ComplexACK Seg 2 (more-follows=true)
    note over Recv: Continue
    Peer->>Recv: ComplexACK Seg 3 (more-follows=true, window_end=3)
    Recv-->>Peer: SegmentACK(seq=3)
    note over Recv: SendSegAck — window_start advances to 4

    Peer->>Recv: ComplexACK Seg 4 (more-follows=false ← last)
    Recv-->>Peer: SegmentACK(seq=4)
    note over Recv: Complete — payload reassembled\nfree_slot → EventResponse
```

---

## How the Stack drives both state machines

Both `SendSegState` and `RecvSegState` are stored in `HashMap`s keyed by
`(BacnetAddr, invoke_id)`. The `Stack` accesses them from:

- **`handle_received`** — routes `SegmentACK` PDUs to `SendSegState::handle_seg_ack`; routes segmented `ComplexACK` fragments to `RecvSegState::accept_fragment`.
- **`handle_tick`** — checks `SendSegState::is_window_timed_out` and calls `retransmit_window`; checks `RecvSegState::is_timed_out` and emits `EventAbort`.

The placeholder `InFlightSlot` (with `request_bytes = []` and
`next_retry_at = ∞`) guards the invoke ID for the lifetime of the send-side
segmentation, preventing the unsegmented retry loop from interfering.
