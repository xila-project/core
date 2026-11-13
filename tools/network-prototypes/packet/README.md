# Packet backend experiment (#413, Approach A)

This independent Cargo workspace connects smoltcp 0.13 to a local **passt**
process through a real Ethernet `Device`. No product dependency, production
crate, root manifest, TAP device, route or firewall is changed. The binary starts
three loopback fixtures and proves TCP echo, UDP echo and a `dns::Socket` A lookup
through smoltcp and passt. It never contacts Internet endpoints, changes the
host resolver, installs software or requests privileges.

## Reproduce

Use an installed passt or extract a distribution package into scratch storage.
For the tested Debian 13 amd64 environment:

```sh
mkdir -p /tmp/xila-passt
curl --fail --location \
  https://deb.debian.org/debian/pool/main/p/passt/passt_0.0~git20250503.587980c-2+deb13u1_amd64.deb \
  -o /tmp/xila-passt/passt.deb
dpkg-deb -x /tmp/xila-passt/passt.deb /tmp/xila-passt/root
export PASST_BIN=/tmp/xila-passt/root/usr/bin/passt
source /workspace/.xila-tools/env.sh  # Only in the managed Xila workspace.
export CARGO_TARGET_DIR=/tmp/network413-packet-target
cargo test --manifest-path tools/network-prototypes/packet/Cargo.toml
cargo test --manifest-path tools/network-prototypes/packet/Cargo.toml \
  --test local_smoke -- --ignored --nocapture
cargo run --manifest-path tools/network-prototypes/packet/Cargo.toml -- "$PASST_BIN"
cargo clippy --manifest-path tools/network-prototypes/packet/Cargo.toml \
  --all-targets -- -D warnings
```

passt's normal unprivileged sandbox creates its own user/mount/IPC/UTS/PID
namespaces. Hosts disabling these operations may reject helper startup; this
prototype preserves the sandbox and reports the error. No network namespace or
host interface is created by this harness. The DNS fixture binds 127.0.0.2:53;
it requires an available port and a host allowing ordinary low-port binds
(`net.ipv4.ip_unprivileged_port_start=0` here). Permission/address-in-use errors
are reported before helper startup. Do not use sudo to work around a blocked
fixture. Fixtures and helper are stopped, joined/reaped, and the private socket
directory removed on ordinary success/error return. External SIGKILL prevents
Rust cleanup.

## Transport and resources

The socket protocol is **four-byte big-endian length + Ethernet frame**.
`PacketDevice` preserves partial headers, payloads and writes across nonblocking
operations. Lengths outside 14..=1514 fail before payload processing. Errors
latch a terminal failure and disable transmit tokens. On orderly EOF between
frames, accepted complete RX frames drain with discard-only reply tokens; no
reply is queued or sent to the closed transport. Malformed or truncated input
remains fail-closed, including withholding previously queued valid frames.

RX and TX rings allocate once: eight fixed frame slots each, plus one inline
partial RX frame. Ethernet payload MTU is 1500 (1514 with header; no FCS). Full
RX queues stop reading; full TX queues withhold tokens. Each pump permits at
most 64 read and 64 write attempts, including interrupted attempts.
`Device::receive` drives the nonblocking pump, so regular smoltcp polling drives
I/O. Errors remain available through `error()`; the `Device` trait has no error
return channel.

Measured x86_64 storage:

| Storage | Bytes |
| --- | ---: |
| Device inline state | 1,616 |
| Two rings: 16 slots × 1,528 bytes | 24,448 |
| Total device (`storage_bytes()`) | **26,064** |
| Smoke TCP application buffers | 8,192 |
| Smoke UDP data buffers | 4,096 |

These exclude socket metadata, DNS/interface/socket-set objects, allocator
overhead, transient frame/token stack values, kernel buffers and the helper.
The diagnostic `pop_frame` allocates a copy; smoltcp receive tokens do not.
Packet copies and the process boundary remain. Three local fixture threads are
smoke instrumentation and would not belong to a real driver.

One run (uid 1000, Debian 13, debug Rust executable):

```text
PASS local TCP echo (21 bytes), UDP echo (21 bytes), DNS fixture.test = 203.0.113.9
elapsed_ms=1005 polls=938 packet_device_storage_bytes=26064 queue_capacity=8 frame_max=1514
helper_VmSize: 205324 kB
helper_VmRSS:   15728 kB
helper_Threads: 1
```

Helper numbers are a single end-of-workload `/proc` snapshot, not peak RSS or
allocator consumption. Virtual address size is not committed physical memory.
This small workload establishes neither throughput nor steady-state CPU cost,
latency distribution or scaling. The smoke sleeps 1 ms between polls; smoltcp
sends its first DNS query after roughly one second here. This cadence is
prototype-only and does not establish event-driven readiness.

## Validation and integration

Twelve transport/Device tests cover fragmentation/coalescing, short writes
interleaved with WouldBlock, bounded queues, token backpressure, malformed
lengths (including after a valid frame), EOF/truncation and terminal errors.
Orderly EOF regressions verify final-frame delivery, draining with a full TX
queue, discard-only replies, and continued fail-closed behavior on truncation.
The opt-in smoke checks actual TCP/UDP/DNS responses, helper reaping and socket
directory cleanup. Initial framing tests failed before transport implementation;
the smoke also timed out without I/O driven from the Device poll contract.

Integration would return this Ethernet device and a controller reporting
`InterfaceKind::Ethernet`; configure 10.0.2.15/24, gateway 10.0.2.2 and DNS
10.0.2.3 instead of existing TAP constants. Own helper startup/failure/shutdown,
private socket storage and configuration in the native driver lifecycle.
The existing runner's 20 ms polling can drive the device, with possible latency
cost. Production work needs readiness wakeups, deliberate batching, failure and
restart reporting, cancellation/reconnect policy and load/CPU/memory benchmarks.
IPv6, incoming listeners/port forwarding, multiple interfaces and cross-platform
builds are unverified. This helper version supports Linux.

passt and libslirp open ordinary host sockets; neither implements HTTP proxy
tunneling just because `HTTP_PROXY` is set. This environment allows external
traffic only through the configured HTTP proxy, so arbitrary external TCP/UDP
and DNS were not tested.

## Candidate comparison and references

passt keeps the Rust adapter small, requires only the C library at runtime and
is GPL-2.0-or-later. Shipping the helper entails its distribution/license/source
obligations; this crate does not link its code. No binary is stored here.
The authoritative [Debian passt source][passt] contains upstream `tap.c`
framing, `passt.1` options, `isolation.c` sandbox and README platform/dependency
statements.

libslirp is an alternative for broader platform support or an in-process
backend. Its [4.8.0 source][slirp] declares BSD-3-Clause (preserve `COPYRIGHT`
notices), depends on GLib, and has Windows and Darwin build branches.
`src/libslirp.h` exposes complete Ethernet frames through `slirp_input` and
`SlirpCb.send_packet`. Integration requires C FFI, clock/timer callbacks and
`slirp_pollfds_fill` → host poll → `slirp_pollfds_poll` ownership. No libslirp
implementation or runtime comparison is included here.

[passt]: https://deb.debian.org/debian/pool/main/p/passt/passt_0.0~git20250503.587980c.orig.tar.xz
[slirp]: https://deb.debian.org/debian/pool/main/libs/libslirp/libslirp_4.8.0.orig.tar.bz2
