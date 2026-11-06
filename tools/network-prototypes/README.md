# Unprivileged networking experiments (#413)

Initial evidence for the issue's architecture gate. These are **standalone
native-only experiments**, not production backends. Root Cargo dependencies,
MCU/browser builds, TAP support and the runtime's `network=none` default are
unchanged. No native runtime flag or automatic host-network access is added.

## What was demonstrated

| | A: packet Device + passt | B: host sockets |
| --- | --- | --- |
| Network path | smoltcp TCP/UDP/DNS → Ethernet Device → helper → host sockets | Xila endpoint/error types → host socket wrapper → shared async-io reactor |
| Local validation | TCP echo, UDP echo, A lookup through actual passt | TCP/UDP echo, host `localhost` lookup, refused connection, half-close, cancellation and quota release |
| IPv6 | Not implemented/tested | Separate local TCP and UDP tests pass on this Linux host |
| Product integration | Fits `Manager::mount_interface(Device, controller)` shape; not installed | API-shaped proof only; not integrated with `Manager` |
| Container observation | Helper startup denied by user-namespace sandbox restriction | TCP/UDP + host name lookup passed as UID 1000 with capabilities dropped |
| Further readiness work | Prototype uses bounded nonblocking pumps; wake-driven integration still required | Event-driven; measured reactor allocations must be addressed before production adoption |

TCP/UDP fixtures bind only loopback. The packet experiment uses a synthetic DNS
fixture at 127.0.0.2:53; the host experiment uses the host resolver's `localhost`
entry. These are different DNS paths, not a shared DNS benchmark. No Internet
connectivity or public DNS was tested under this environment's proxy policy.

## Resource observations

These metrics describe different storage classes. They are **not directly
comparable total-memory measurements** and do not establish an MCU budget.

- A's Rust packet Device reserves 26,064 bytes for inline state and eight RX +
  eight TX frame slots. Smoke TCP/UDP buffers add 12,288 bytes; metadata,
  DNS/interface state, token stack values and kernel buffers are excluded.
- A's helper used 15,728 KiB RSS (~15.4 MiB) in a single tiny-workload snapshot, one
  thread, with about 200.5 MiB virtual address space. RSS is not allocator peak;
  virtual space is not committed RAM. See [packet details](packet/README.md).
- B's warmed native client used about 61.3 KB additional live Rust allocation
  including shared-reactor startup, plus one reactor thread. This excludes
  kernel socket buffers, libc allocation and thread stack reservations.
- B's optional resolver requests a 128 KiB worker stack and bounds pending jobs;
  it does not impose a hard memory bound on libc/NSS work. In-flight libc lookup
  cannot be interrupted and worker shutdown is not immediate.
- B's probe observed 200 Rust allocation requests during 100 TCP+UDP echo pairs
  (21 bytes each), with 160 extra peak live bytes beyond the warmed level.
  A container run observed 193 requests: readiness timing affects the count.
  **The current reactor does not satisfy zero-allocation steady-state I/O.**
- The host probe's 6.44 ms/100-pair observation and A's roughly 1 s smoke duration
  are different workloads (the latter includes smoltcp DNS startup). They must
  not be presented as a performance comparison. Throughput/scaling and a common
  repeated-workload CPU/RSS/stack baseline remain to be measured.

## Container check

Used the locally available `ghcr.io/xila-project/development-environment:main`
image, normal Docker isolation, `--user 1000:1000 --cap-drop=ALL --read-only`,
read-only mounts of the binaries/fixtures, and a 64 MiB writable `/tmp` tmpfs.
`--network=none` limited the experiment to in-container loopback. No privileged
flag, host networking, TUN device, route/firewall change or security-profile
relaxation was used. This checks local unprivileged behavior only, not outbound
container egress or Xila's runtime-level `network=none` policy.

Host command shape (substitute the built binary path):

```sh
docker run --rm --network=none --user 1000:1000 --cap-drop=ALL --read-only \
  --tmpfs /tmp:rw,nosuid,nodev,size=64m \
  --mount type=bind,src=/absolute/path/to/release/probe,dst=/probe,readonly \
  --mount type=bind,src=/absolute/path/to/host/fixtures.py,dst=/fixtures.py,readonly \
  ghcr.io/xila-project/development-environment:main python3 /fixtures.py /probe
```

The corresponding packet command mounted its binary at `/probe` and passt at
`/passt`, invoking `/probe /passt`. It failed with `Couldn't create user
namespace: Operation not permitted`. Passt's own sandbox namespaces differ from
modifying the host network namespace. This is an observed restriction of this
container profile, not a claim that passt is universally incompatible with OCI.
The helper's sandbox was not disabled to make the test pass.

## Recommendation and next implementation boundary

Investigate the **host-provider boundary next for the default native/container
case**, because it avoids a helper sandbox dependency and passed the constrained
container check. Keep passt as a useful alternative for smoltcp fidelity and
explicit simulation. This is a provisional direction, not a production rollout
or a claim that the issue is complete.

Before adopting a host provider:

1. Replace smoltcp-specific public closure/handle assumptions with an explicit
   transport-neutral operation boundary; decide what remains low-level-only.
2. Resolve `flush` semantics: host enqueue/flush does not promise remote ACK,
   unlike the current smoltcp implementation.
3. Use a bounded/reused reactor event store that meets the allocation gate,
   and define cancellation, runtime shutdown, deadlines and resource ownership.
4. Define host DNS behavior, limits and shutdown. A maximum returned-answer
   count does not bound `getaddrinfo`'s temporary allocation.
5. Test actual HTTP/HTTPS devices through the chosen boundary; retain TCP/UDP,
   refusal/timeout/reset, multiple flows and teardown tests. Then validate normal
   container egress, namespace policy and independent IPv6 behavior.
6. Keep `--network=none` authoritative and TAP opt-in. Unsupported host interface
   controls must return an explicit error rather than fake route/IP changes.

## Validation and sources

Each subdirectory contains its own manifest, lockfile, tests and reproduction
commands. Run tests/Clippy using `--manifest-path`; they are not root workspace
members. Packet framing tests include partial/coalesced I/O, bounded queues,
malformed/truncated input and final complete frames before orderly EOF. The live
helper smoke is explicit opt-in and checks helper reaping/socket cleanup.

The independent review identified EOF draining in the packet prototype and
missing EOF assertions/deadlines in host test loops; these have regression or
test-harness fixes. Review also clarified DNS memory and measurement limits.

See [host observations](host/README.md) and [packet source/license analysis](packet/README.md)
for passt/libslirp references and portability/distribution constraints. No helper
binary, downloaded distribution package, container image or build output is
stored in the repository.
