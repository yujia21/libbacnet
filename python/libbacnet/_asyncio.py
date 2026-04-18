"""
BACnet/IP asyncio application layer.

Two public classes:

* ``BacnetProtocol`` — low-level ``asyncio.DatagramProtocol`` that owns the
  UDP socket, drives the sans-IO ``Stack``, and emits ``BacnetEvent`` objects
  to an async queue.

* ``BacnetClient`` — high-level API built on top of ``BacnetProtocol``.
  Exposes ``async def`` methods for confirmed services and device discovery.

Typical usage::

    async def main():
        async with BacnetClient(local_addr=("0.0.0.0", 47808)) as client:
            devices = await client.who_is(timeout=3.0)
            for dev in devices:
                pv = await client.read_property(
                    addr=dev.src,
                    obj_id=ObjectIdentifier(object_type=8, instance=dev.message.device_id.instance),
                    prop_id=85,
                )
"""

from __future__ import annotations

import asyncio
import logging
import struct
import time
from typing import TYPE_CHECKING, Self

import libbacnet as _lib  # re-exported symbols from libbacnet.libbacnet

if TYPE_CHECKING:
    from collections.abc import Callable

_logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# BacnetProtocol
# ---------------------------------------------------------------------------


class BacnetProtocol(asyncio.DatagramProtocol):
    """
    Low-level ``asyncio.DatagramProtocol`` wrapping the sans-IO ``Stack``.

    ``BacnetProtocol`` is the glue between the operating system's UDP socket
    and the pure Rust sans-IO ``Stack``.  It:

    * Converts each received UDP datagram into an ``InputReceived`` and feeds
      it to the stack via :meth:`datagram_received`.
    * Calls ``transport.sendto`` for every ``OutputTransmit`` the stack emits.
    * Maintains a single ``loop.call_later`` handle so the stack's retry and
      segmentation timers fire on time (``OutputDeadline`` → reschedule).
    * Puts every ``BacnetEvent`` from the stack into :attr:`events` (an
      ``asyncio.Queue``) *and* calls the optional ``on_event`` callback.

    Typical direct usage (advanced)::

        loop = asyncio.get_running_loop()
        protocol = BacnetProtocol(on_event=my_handler)
        transport, _ = await loop.create_datagram_endpoint(
            lambda: protocol,
            local_addr=("0.0.0.0", 47808),
            allow_broadcast=True,
        )

    For the common case use :class:`BacnetClient` instead.

    Attributes:
        events: Unbounded ``asyncio.Queue`` of ``BacnetEvent`` objects.
            Consumers can ``await protocol.events.get()`` in a separate task.

    Args:
        config: Optional :class:`StackConfig` to tune timeouts, retries, and
            APDU / segment buffer sizes.  Defaults to ``StackConfig()``.
        on_event: Optional synchronous callback invoked for every
            ``BacnetEvent``.  Called in addition to the :attr:`events` queue.

    """

    def __init__(
        self,
        config: _lib.StackConfig | None = None,
        on_event: Callable[[object], None] | None = None,
    ) -> None:
        self._stack = _lib.Stack(config=config)
        self._transport: asyncio.BaseTransport | None = None
        self._tick_handle: asyncio.TimerHandle | None = None
        # async queue for event consumers (unbounded)
        self.events: asyncio.Queue[object] = asyncio.Queue()
        self._on_event = on_event

    # ------------------------------------------------------------------
    # asyncio.DatagramProtocol interface
    # ------------------------------------------------------------------

    def connection_made(self, transport: asyncio.BaseTransport) -> None:  # type: ignore[override]  # noqa: D401
        """
        Called by asyncio when the UDP socket is ready.

        Stores the transport and performs an initial ``Tick`` so any startup
        deadline is set immediately.

        Args:
            transport: The asyncio transport for the UDP socket.

        """  # noqa: D401
        self._transport = transport
        _logger.debug("UDP transport ready")
        self._do_tick()

    def datagram_received(self, data: bytes, addr: tuple[str, int]) -> None:  # noqa: D401
        """
        Called by asyncio when a UDP datagram arrives.

        Wraps ``data`` in an ``InputReceived`` and drives it through the stack.
        All resulting outputs (transmits, events, deadline updates) are handled
        immediately and synchronously.

        Args:
            data: Raw UDP payload bytes.
            addr: ``(ip_address_str, port)`` of the sender.

        """  # noqa: D401
        _logger.debug("datagram_received %d bytes from %s:%d", len(data), addr[0], addr[1])
        src = _lib.BacnetAddr(addr[0], addr[1])
        inp = _lib.InputReceived(data=list(data), src=src)
        self._handle_outputs(self._stack.process(inp))

    def error_received(self, exc: Exception) -> None:  # noqa: ARG002, D401
        """
        Called by asyncio on a non-fatal UDP error (e.g. ICMP port unreachable).

        The sans-IO stack is unaffected.  Subclasses may override to log or
        propagate the error.

        Args:
            exc: The exception from the OS.

        """  # noqa: D401
        _logger.warning("error_received: %s", exc)

    def connection_lost(self, exc: Exception | None) -> None:  # noqa: D401
        """
        Called by asyncio when the transport is closed.

        Cancels the pending tick handle so no further callbacks fire.

        Args:
            exc: Exception if the connection was lost due to an error,
                ``None`` for a clean close.

        """  # noqa: D401
        if self._tick_handle is not None:
            self._tick_handle.cancel()
            self._tick_handle = None
        _logger.debug("connection_lost exc=%s", exc)

    # ------------------------------------------------------------------
    # Public helpers
    # ------------------------------------------------------------------

    def send_input(self, inp: object) -> list[object]:
        """
        Drive an ``Input`` into the stack and handle all resulting outputs.

        This is the primary method for injecting inputs outside of the normal
        UDP receive path (e.g. ``InputSend`` for confirmed requests).

        All outputs are handled synchronously: transmits are sent via the
        transport, events are enqueued and/or dispatched to ``on_event``, and
        deadline updates reschedule the tick timer.

        Args:
            inp: Any ``Input`` variant — ``InputReceived``, ``InputTick``,
                or ``InputSend``.

        Returns:
            The raw list of ``Output`` objects produced by the stack.  Useful
            for testing without a real transport attached.

        Raises:
            InvokeIdExhaustedError: Propagated from the stack when all 256
                invoke IDs for the destination are in use.

        """
        outputs = self._stack.process(inp)
        self._handle_outputs(outputs)
        return outputs  # type: ignore[return-value]

    def add_event_listener(self, callback: Callable[[object], None]) -> None:
        """
        Register a synchronous callback invoked for every ``BacnetEvent``.

        The callback is called in addition to the :attr:`events` queue.
        Replaces any previously registered callback.

        Args:
            callback: A callable that accepts a single ``BacnetEvent`` argument.
                ``EventResponse``, ``EventTimeout``, ``EventAbort``,
                ``EventError``, or ``EventUnconfirmedReceived``.

        """
        self._on_event = callback

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _handle_outputs(self, outputs: list[object]) -> None:
        for out in outputs:
            if isinstance(out, _lib.OutputTransmit):
                self._transmit(out)
            elif isinstance(out, _lib.OutputEvent):
                self._emit_event(out.event)
            elif isinstance(out, _lib.OutputDeadline):
                self._reschedule_tick(out.deadline)

    def _transmit(self, out: _lib.OutputTransmit) -> None:
        if self._transport is not None:
            dest = (out.dest.addr, out.dest.port)
            _logger.debug("transmit %d bytes to %s:%d", len(out.data), dest[0], dest[1])
            self._transport.sendto(bytes(out.data), dest)  # type: ignore[attr-defined]

    def _emit_event(self, event: object) -> None:
        self.events.put_nowait(event)
        if self._on_event is not None:
            self._on_event(event)

    def _reschedule_tick(self, deadline: float) -> None:
        """
        Schedule (or reschedule) the next ``Tick`` call for ``deadline``.

        Args:
            deadline: Absolute monotonic timestamp for the next tick.

        """
        if self._tick_handle is not None:
            self._tick_handle.cancel()
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            return  # no running loop (e.g. sync tests) — skip scheduling
        delay = max(0.0, deadline - time.monotonic())
        self._tick_handle = loop.call_later(delay, self._do_tick)

    def _do_tick(self) -> None:
        self._tick_handle = None
        now = time.monotonic()
        inp = _lib.InputTick(now=now)
        outputs = self._stack.process(inp)
        self._handle_outputs(outputs)


# ---------------------------------------------------------------------------
# BacnetClient
# ---------------------------------------------------------------------------


class BacnetClient:
    """
    High-level asyncio BACnet/IP client.

    ``BacnetClient`` is the recommended entry point for most applications.
    It manages a ``BacnetProtocol`` instance (and thus a UDP socket) and
    exposes ``async def`` methods for each supported confirmed service plus
    device discovery.

    The confirmed-service methods (:meth:`read_property`,
    :meth:`read_property_multiple`, :meth:`write_property`) each:

    1. Encode the request and hand it to the stack via ``InputSend``.
    2. Register an ``asyncio.Future`` keyed by ``(dest_addr, dest_port, invoke_id)``.
    3. Await the future, which is resolved (or rejected) when the stack emits
       the corresponding ``EventResponse``, ``EventTimeout``, ``EventAbort``,
       or ``EventError``.

    Usage::

        async with BacnetClient() as client:
            devices = await client.who_is(wait=3.0)
            for dev in devices:
                addr = dev.src
                oid = ObjectIdentifier(object_type=8, instance=dev.message.device_id.instance)
                res = await client.read_property(addr=addr, obj_id=oid, prop_id=85)

    Args:
        config: Optional :class:`StackConfig` to tune timeouts, retries, and
            APDU / segment buffer sizes.  Defaults to ``StackConfig()``.
        local_addr: ``(host, port)`` to bind the local UDP socket.
            Defaults to ``("0.0.0.0", 47808)`` (all interfaces, standard
            BACnet/IP port).  Pass port ``0`` to let the OS pick a free port
            (useful in tests).

    """

    def __init__(
        self,
        config: _lib.StackConfig | None = None,
        local_addr: tuple[str, int] = ("0.0.0.0", 47808),
    ) -> None:
        self._config = config
        self._local_addr = local_addr
        self._protocol: BacnetProtocol | None = None
        self._transport: asyncio.BaseTransport | None = None
        # futures keyed by (dest_addr, dest_port, invoke_id)
        self._pending: dict[tuple[str, int, int], asyncio.Future[bytes]] = {}  # bytes = raw ACK payload before decode
        self._who_is_collector: list[object] | None = None

    async def __aenter__(self) -> Self:
        """Open the UDP socket and return self."""
        loop = asyncio.get_running_loop()
        protocol = BacnetProtocol(config=self._config)
        protocol.add_event_listener(self._on_event)
        transport, _ = await loop.create_datagram_endpoint(
            lambda: protocol,
            local_addr=self._local_addr,
            allow_broadcast=True,
        )
        self._transport = transport
        self._protocol = protocol
        _logger.info("BacnetClient opened on %s:%d", self._local_addr[0], self._local_addr[1])
        return self

    async def __aexit__(self, exc_type: object, exc_val: object, exc_tb: object) -> None:
        """Cancel pending futures and close the UDP socket."""
        for fut in self._pending.values():
            fut.cancel()
        self._pending.clear()
        if self._transport is not None:
            self._transport.close()
            self._transport = None
            self._protocol = None
            _logger.info("BacnetClient closed")

    # ------------------------------------------------------------------
    # read_property
    # ------------------------------------------------------------------

    async def read_property(
        self,
        addr: _lib.BacnetAddr,
        obj_id: _lib.ObjectIdentifier,
        prop_id: int,
        array_index: int | None = None,
    ) -> _lib.ReadPropertyResult:
        """
        Send a ReadProperty confirmed request and return the decoded result.

        Args:
            addr: Destination device address.
            obj_id: Object identifier (type + instance).
            prop_id: Property identifier (e.g. ``85`` for present-value).
            array_index: Optional array index.  ``None`` means read the whole
                property.

        Returns:
            :class:`ReadPropertyResult` with ``object_id``, ``property_id``,
            ``array_index``, and a typed ``value`` (a ``PropertyValue*``
            instance).

        Raises:
            BacnetTimeoutError: No response within the retry budget.
            BacnetError: The server sent an Error or Abort PDU, or the
                response payload could not be decoded.
            InvokeIdExhaustedError: All 256 invoke IDs for ``addr`` are busy.
            RuntimeError: Client has not been opened via ``async with``.

        """
        svc = _lib.ServiceReadProperty(
            object_id=obj_id,
            property_id=prop_id,
            array_index=array_index,
        )
        raw = await self._send_confirmed(addr, svc)
        return _lib.decode_read_property(raw)

    # ------------------------------------------------------------------
    # read_property_multiple
    # ------------------------------------------------------------------

    async def read_property_multiple(
        self,
        addr: _lib.BacnetAddr,
        request_list: list[_lib.ReadAccessSpec],
    ) -> _lib.ReadPropertyMultipleResult:
        """
        Send a ReadPropertyMultiple confirmed request and return the decoded result.

        Args:
            addr: Destination device address.
            request_list: List of :class:`ReadAccessSpec` objects, each
                specifying an object and one or more property references.

        Returns:
            :class:`ReadPropertyMultipleResult` with a list of
            :class:`ObjectResult` objects, each containing an ``object_id``
            and a list of :class:`PropertyResult` objects.  Each
            ``PropertyResult.value`` is either a typed ``PropertyValue*``
            instance or a :class:`BacnetPropertyError`.

        Raises:
            BacnetTimeoutError: No response within the retry budget.
            BacnetError: The server sent an Error or Abort PDU, or the
                response payload could not be decoded.
            InvokeIdExhaustedError: All 256 invoke IDs for ``addr`` are busy.
            RuntimeError: Client has not been opened via ``async with``.

        """
        svc = _lib.ServiceReadPropertyMultiple(specs=request_list)
        raw = await self._send_confirmed(addr, svc)
        return _lib.decode_read_property_multiple(raw)

    # ------------------------------------------------------------------
    # write_property
    # ------------------------------------------------------------------

    async def write_property(
        self,
        addr: _lib.BacnetAddr,
        obj_id: _lib.ObjectIdentifier,
        prop_id: int,
        value: object,
        array_index: int | None = None,
        priority: int | None = None,
    ) -> None:
        """
        Send a WriteProperty confirmed request.

        Args:
            addr: Destination device address.
            obj_id: Object identifier (type + instance).
            prop_id: Property identifier.
            value: A ``PropertyValue*`` instance (e.g. ``PropertyValueReal(23.5)``).
            array_index: Optional array index for array properties.
            priority: Optional write priority (1–16).  ``None`` uses the
                device's default priority.

        Returns:
            ``None`` on success (SimpleACK carries no payload).

        Raises:
            BacnetTimeoutError: No response within the retry budget.
            BacnetError: The server sent an Error or Abort PDU.
            InvokeIdExhaustedError: All 256 invoke IDs for ``addr`` are busy.
            RuntimeError: Client has not been opened via ``async with``.

        """
        svc = _lib.ServiceWriteProperty(
            object_id=obj_id,
            property_id=prop_id,
            value=value,
            array_index=array_index,
            priority=priority,
        )
        await self._send_confirmed(addr, svc)

    # ------------------------------------------------------------------
    # who_is
    # ------------------------------------------------------------------

    async def who_is(
        self,
        addr: _lib.BacnetAddr | None = None,
        low: int | None = None,
        high: int | None = None,
        wait: float = 3.0,
    ) -> list[object]:
        """
        Broadcast a Who-Is and collect I-Am responses.

        Sends a Who-Is unconfirmed request to ``addr`` (or the global broadcast
        address) and then waits ``wait`` seconds for I-Am responses.  All
        ``EventUnconfirmedReceived`` events carrying ``UnconfirmedIAm`` messages
        that arrive during the window are returned.

        Args:
            addr: Destination broadcast address.  Defaults to
                ``255.255.255.255:47808``.
            low: Optional lower bound of the device instance range filter.
                Both ``low`` and ``high`` must be provided together.
            high: Optional upper bound of the device instance range filter.
            wait: Collection window duration in seconds.

        Returns:
            List of ``EventUnconfirmedReceived`` events.  Each event has a
            ``src`` (:class:`BacnetAddr`) and a ``message`` (:class:`UnconfirmedIAm`).

        Raises:
            RuntimeError: Client has not been opened via ``async with``.

        """
        if self._protocol is None:
            msg = "BacnetClient must be used as an async context manager"
            raise RuntimeError(msg)

        broadcast = addr or _lib.BacnetAddr("255.255.255.255", 47808)
        self._who_is_collector = []

        self._send_who_is(broadcast, low, high)

        await asyncio.sleep(wait)

        collected = self._who_is_collector or []
        self._who_is_collector = None
        return [
            e
            for e in collected
            if isinstance(e, _lib.EventUnconfirmedReceived) and isinstance(e.message, _lib.UnconfirmedIAm)
        ]

    # ------------------------------------------------------------------
    # who_is_router_to_network
    # ------------------------------------------------------------------

    async def who_is_router_to_network(
        self,
        network: int | None = None,
        wait: float = 3.0,
    ) -> list[object]:
        """
        Send a WhoIsRouterToNetwork and collect IAmRouterToNetwork responses.

        Args:
            network: Optional target network number.  If ``None``, asks for
                all routers on the local IP network.
            wait: Collection window duration in seconds.

        Returns:
            List of ``EventUnconfirmedReceived`` events whose ``message`` is an
            ``UnconfirmedIAmRouterToNetwork``.

        Raises:
            RuntimeError: Client has not been opened via ``async with``.

        """
        if self._protocol is None:
            msg = "BacnetClient must be used as an async context manager"
            raise RuntimeError(msg)

        broadcast = _lib.BacnetAddr("255.255.255.255", 47808)
        self._who_is_collector = []
        self._send_who_is_router(broadcast, network)

        await asyncio.sleep(wait)

        collected = self._who_is_collector or []
        self._who_is_collector = None
        return [
            e
            for e in collected
            if isinstance(e, _lib.EventUnconfirmedReceived)
            and isinstance(e.message, _lib.UnconfirmedIAmRouterToNetwork)
        ]

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    async def _send_confirmed(
        self,
        addr: _lib.BacnetAddr,
        service: object,
    ) -> bytes:
        """
        Send a confirmed request and await the response future.

        Args:
            addr: Destination device address.
            service: An encoded ``Service*`` instance.

        Returns:
            Raw service-data bytes from the ACK payload.

        Raises:
            RuntimeError: Client has not been opened via ``async with``.

        """
        if self._protocol is None:
            msg = "BacnetClient must be used as an async context manager"
            raise RuntimeError(msg)

        loop = asyncio.get_running_loop()
        fut: asyncio.Future[bytes] = loop.create_future()

        inp = _lib.InputSend(service=service, dest=addr)
        outputs = self._protocol.send_input(inp)

        invoke_id = self._extract_invoke_id(outputs)
        key = (addr.addr, addr.port, invoke_id)
        self._pending[key] = fut
        _logger.debug("confirmed request invoke_id=%d to %s:%d", invoke_id, addr.addr, addr.port)

        try:
            return await fut
        finally:
            self._pending.pop(key, None)

    def _extract_invoke_id(self, outputs: list[object]) -> int:
        """
        Extract invoke_id from the first Transmit output's APDU bytes.

        Args:
            outputs: List of ``Output`` objects from the stack.

        Returns:
            The invoke ID byte extracted from the APDU.

        Raises:
            RuntimeError: No Transmit output was found.

        """
        for out in outputs:
            if isinstance(out, _lib.OutputTransmit):
                data = out.data
                bvlc_len = 4
                npdu_ctrl = data[bvlc_len + 1]
                npdu_overhead = 2
                if npdu_ctrl & 0x20:
                    npdu_overhead += 2 + data[bvlc_len + 4] + 1
                apdu_start = bvlc_len + npdu_overhead
                return data[apdu_start + 2]  # type: ignore[return-value]
        msg = "No Transmit output found"
        raise RuntimeError(msg)

    def _on_event(self, event: object) -> None:  # noqa: C901
        """
        Handle a BacnetEvent from the protocol.

        Args:
            event: A ``BacnetEvent`` variant from the Rust stack.

        """
        if self._who_is_collector is not None:
            if isinstance(event, _lib.EventUnconfirmedReceived):
                self._who_is_collector.append(event)
            return

        if isinstance(event, _lib.EventResponse):
            src = self._find_pending_src(event.invoke_id)
            if src is not None:
                key = (*src, event.invoke_id)
                fut = self._pending.get(key)
                if fut is not None and not fut.done():
                    fut.set_result(bytes(event.payload))

        elif isinstance(event, _lib.EventTimeout):
            src = self._find_pending_src(event.invoke_id)
            if src is not None:
                key = (*src, event.invoke_id)
                fut = self._pending.get(key)
                if fut is not None and not fut.done():
                    fut.set_exception(
                        _lib.BacnetTimeoutError("No response within retry budget"),
                    )

        elif isinstance(event, _lib.EventAbort):
            src = self._find_pending_src(event.invoke_id)
            if src is not None:
                key = (*src, event.invoke_id)
                fut = self._pending.get(key)
                if fut is not None and not fut.done():
                    fut.set_exception(
                        _lib.BacnetError(f"Abort reason={event.reason}"),
                    )

        elif isinstance(event, _lib.EventError):
            src = self._find_pending_src(event.invoke_id)
            if src is not None:
                key = (*src, event.invoke_id)
                fut = self._pending.get(key)
                if fut is not None and not fut.done():
                    fut.set_exception(_lib.BacnetError(event.message))

    def _find_pending_src(self, invoke_id: int) -> tuple[str, int] | None:
        """
        Return the (addr, port) key that matches invoke_id in _pending.

        Args:
            invoke_id: The invoke ID to look up.

        Returns:
            ``(addr_str, port)`` tuple if found, ``None`` otherwise.

        """
        for addr_str, port, iid in self._pending:
            if iid == invoke_id:
                return addr_str, port
        return None

    def _send_who_is(
        self,
        dest: _lib.BacnetAddr,
        low: int | None,
        high: int | None,
    ) -> None:
        """
        Build and transmit a Who-Is NPDU directly (no invoke ID).

        Args:
            dest: Destination broadcast address.
            low: Optional lower device instance range bound.
            high: Optional upper device instance range bound.

        """
        if low is not None and high is not None:
            svc_data = _encode_context_uint(0, low) + _encode_context_uint(1, high)
        else:
            svc_data = b""

        apdu = bytes([0x10, 0x08]) + svc_data
        npdu = bytes([0x01, 0x20, 0xFF, 0xFF, 0x00, 0xFF]) + apdu
        bvlc_len = 4 + len(npdu)
        bvlc = bytes([0x81, 0x0B]) + struct.pack(">H", bvlc_len) + npdu

        if self._protocol is not None and self._protocol._transport is not None:
            self._protocol._transport.sendto(  # type: ignore[attr-defined]
                bvlc,
                (dest.addr, dest.port),
            )

    def _send_who_is_router(
        self,
        dest: _lib.BacnetAddr,
        network: int | None,
    ) -> None:
        """
        Build and transmit a WhoIsRouterToNetwork NPDU.

        Args:
            dest: Destination broadcast address.
            network: Optional target network number.

        """
        msg_data = struct.pack(">H", network) if network is not None else b""
        npdu = bytes([0x01, 0xA0, 0x00]) + msg_data
        bvlc_len = 4 + len(npdu)
        bvlc = bytes([0x81, 0x0B]) + struct.pack(">H", bvlc_len) + npdu

        if self._protocol is not None and self._protocol._transport is not None:
            self._protocol._transport.sendto(  # type: ignore[attr-defined]
                bvlc,
                (dest.addr, dest.port),
            )


def _encode_context_uint(tag: int, value: int) -> bytes:
    """
    Encode a BACnet context-tagged unsigned integer.

    Args:
        tag: The BACnet context tag number (0–14).
        value: The unsigned integer value to encode.

    Returns:
        Encoded bytes for the context-tagged integer.

    """
    if value < 256:
        return bytes([tag << 4 | 0x09, value])
    if value < 65536:
        return bytes([tag << 4 | 0x0A]) + struct.pack(">H", value)
    return bytes([tag << 4 | 0x0C]) + struct.pack(">I", value)
