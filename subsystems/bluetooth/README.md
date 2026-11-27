# Bluetooth Low Energy

Enable the `bluetooth` feature on `xila` to use this subsystem. The runtime
creates a static `BluetoothDevice`, mounts it at `/devices/bluetooth/hci0`, and
runs `BluetoothDevice::run` on a local task with a Host Controller Interface
(HCI) controller. The runtime supplies the Embassy time driver and critical
section implementation. The mounting user owns the device. Only that user can
open it. Mount as root to reserve the device for a privileged service.

The device accepts one opening at a time. The opening owns the scan, broadcast,
connection, subscriptions, and events. It cannot duplicate its descriptor.
Close cancels work and discards queued events. A new opening can succeed when
the runner finishes cleanup. The runner manages the protocol and the controller
moves HCI packets. Device policy and persistent data belong to the caller.

## Request and event flow

1. Open the device for read and write.
2. Use `GET_CAPABILITIES` and `GET_STATE` to read its capabilities and state.
3. Submit a command with `File::control`. A successful return means the runner
   accepted the command. Read events until its completion, then submit the
   next command.
4. Use `File::read` to receive one `Event` record. A blocking read waits for
   the runner. A nonblocking read returns `WouldBlock` when the queue is empty.
   The VFS readiness API reports whether a record can be read.

All control and event fields have fixed layouts. Read an event into
`Event::as_mutable_bytes` and use `Event::payload` for its typed payload.
The layout uses native byte order, as for other device controls. A failed
completion carries a stable Xila I/O error identifier. A connection handle
belongs to one connection in one opening. Use the handle from the successful
`CONNECT` completion.

The controller can scan, broadcast legacy nonconnectable advertisements, or
hold one central connection. Stop a scan or broadcast before connecting.
Discover services before requesting their characteristics or descriptors.
Characteristic reads and writes use the value handle. Configure notifications
with `SET_NOTIFICATIONS`. Each discovery can return up to eight records.
Each connection permits eight notification subscriptions. The local ATT
(Attribute Protocol) limit is 44 bytes, including headers. The value limit is
41 bytes. A peer can negotiate a lower write limit.

The queue holds 16 records and reserves one slot for a command completion.
When it fills, the device drops new data events and reports the number lost
after the queued records. Use `GET_STATE` to recover the current scan,
broadcast, and connection state. The Bluetooth host also has a bounded
notification queue. Bursts can lose notifications at that layer before they
reach the device queue.

## Simulation

Use the `bluetooth_mock` feature on `xila-drivers-core` to supply an in-memory
controller. It has configurable peers and attribute tables. Call
`Simulation::advance` to make scheduled controller events available. The
simulator can delay an advertisement, connection or attribute response, drop a
connection, or leave a connection request unanswered. The VFS integration
test drives the same host path that a hardware controller would use.
