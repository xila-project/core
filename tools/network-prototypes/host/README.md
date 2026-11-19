# Host-socket experiment (#413, Approach B)

Standalone native-only prototype, not wired into `network::Manager`. It reuses
Xila `IpAddress`, `Port` and `Error` types and maps TCP/UDP operations onto
nonblocking host sockets driven by async-io's shared reactor. No production
manifest, feature, networking default, TAP configuration or embedded API changes.

```sh
cargo test --manifest-path tools/network-prototypes/host/Cargo.toml
cargo clippy --manifest-path tools/network-prototypes/host/Cargo.toml --all-targets -- -D warnings
cargo build --manifest-path tools/network-prototypes/host/Cargo.toml --release
python3 tools/network-prototypes/host/fixtures.py /path/to/release/probe
```

The Python script starts local echo fixtures in a separate process from the
Rust client. It never uses external destinations. Tests have a five-second
client deadline and cover TCP/UDP, refused connect, pending-read cancellation,
quota reclamation, host `localhost` lookup, half-close, and separate IPv6 TCP/UDP.

## Resource and lifecycle model

One provider-level socket quota covers both TCP and UDP, including a connect in
progress. A lease returns the quota on error, cancellation or socket drop. Host
socket buffers remain kernel-owned: the quota is not a RAM bound, and it does not
implement Xila's current per-socket TX/RX buffer-size contract. `flush` here means
host writes are queued, not acknowledged; that differs from current smoltcp
`TcpSocket::flush` and must be resolved before integrating a provider boundary.

Read/write futures use event readiness and standard Rust wakers. No polling
sleep or OS thread per socket is used. async-io starts one shared reactor thread
on this Linux host. This dependency is **not allocation-free** in steady state:
its reactor collects ready wakers into a fresh Vec on each reaction. The memory
probe exposes this cost rather than claiming the #252 hot-path allocation gate
has passed. A production candidate needs a bounded/reused event store or another
reactor integration before adoption.

The optional `Resolver` starts one worker with a requested 128 KiB stack and a
bounded pending-job quota/queue. libc/NSS resolution runs there, never in a
future's `poll`. Cancellation removes the task waker and lets queued work skip
lookup; an in-flight libc call cannot be interrupted. Dropping the resolver
closes the queue, but the detached worker may finish after drop. Production
shutdown deadlines need a separate solution. Sixteen returned addresses is a
result limit, **not a hard DNS memory bound**: libc and `ToSocketAddrs` can build
larger temporary results first. Host libc allocations are not tracked by the
Rust allocator probe. No host resolver configuration is modified.

## Observations (Linux x86_64, release, one tiny-workload run)

100 sequential TCP+UDP echo pairs, 21 bytes each, after one TCP warm-up:

- 6.44 ms total on this host (no throughput or latency distribution claim).
- 200 Rust allocation requests during the measured operations; 160 bytes
  additional peak Rust live allocation beyond the post-warm-up level.
- 61,297 bytes client Rust live-allocation delta from pre-provider baseline,
  including shared reactor initialization; not per socket.
- One additional reactor thread; optional DNS adds another worker and requests
  131,072 bytes of stack reservation. Neither thread stacks nor kernel socket
  memory are included in the Rust allocator counters.
- Release probe executable: 780,928 bytes, unstripped, using this toolchain.

These are observations, not production memory budgets. Compare packet device
storage, client allocator counts, helper RSS and reserved stack as different
metrics. The packet smoke is a different workload and includes DNS startup;
its elapsed time cannot be compared to this loop as a speed ratio.

## Production boundary work still required

The current Xila TCP/UDP/ICMP wrappers expose concrete smoltcp socket closures,
SocketContext handles and interface controls. A host provider cannot implement
these honestly. It needs a transport-neutral operation boundary, explicit
unsupported interface controls, flow shutdown/deadline ownership and consistent
flush/backpressure semantics. This prototype is an API-shaped proof, not an
alternate `Manager` or a second production socket stack. HTTP/HTTPS integration,
container validation, controlled inbound listeners and non-Linux builds remain
follow-up work. Host sockets do not automatically honor HTTP proxy variables;
external egress must continue obeying the configured network policy.
