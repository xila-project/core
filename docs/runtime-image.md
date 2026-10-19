# Xila runtime image

The runtime image is for executing Xila applications. It is separate from the
development environments and contains a native runner plus an immutable,
pre-provisioned LittleFS seed. The runner creates the mutable disk at
`/var/lib/xila/drive.img` the first time it starts. Keep host-shared files in
explicit HostFS mounts; they are not copied into the LittleFS disk.

The image currently targets Linux `amd64` and is published to GitHub Container
Registry as `ghcr.io/xila-project/xila-runtime` after the end-to-end smoke test
passes. Multi-platform publication is not enabled until the runtime and its
dependencies have been validated on each architecture.

## Pull a published image

The `main` branch publishes `latest` and a commit-SHA tag. A Git tag beginning
with `v` publishes the matching version tag and a commit-SHA tag:

```sh
docker pull ghcr.io/xila-project/xila-runtime:latest
docker pull ghcr.io/xila-project/xila-runtime:v1.2.3
```

Use either image in place of `xila-runtime:local` in the commands below.

## Run an application from a read-only host mount

Build the local image with:

```sh
docker build -f Dockerfile.runtime -t xila-runtime:local .
```

Then expose an application directory as a read-only container bind mount and
explicitly map it into the Xila VFS:

```sh
docker run --rm \
  --mount type=bind,src="$PWD",dst=/mnt/xila/application,readonly \
  xila-runtime:local \
  run \
  --disk /var/lib/xila/drive.img \
  --seed /usr/share/xila/base.img \
  --network=none \
  --mount host=/mnt/xila/application,xila=/application,ro \
  -- /binaries/wasm /application/app.wasm
```

The path after `--` is an Xila VFS path. A host path is visible to Xila only
after it has been bind-mounted into the container and explicitly mapped with
`--mount`. Use `readonly` on container input binds and `ro` in the Xila mount
configuration. For data the application needs to write, provide a separate
container bind mount and map it with `rw`:

```sh
mkdir -p "$PWD/output"
docker run --rm \
  --mount type=bind,src="$PWD/input",dst=/mnt/xila/input,readonly \
  --mount type=bind,src="$PWD/output",dst=/mnt/xila/output \
  xila-runtime:local \
  run --disk /var/lib/xila/drive.img --seed /usr/share/xila/base.img --network=none \
  --mount host=/mnt/xila/input,xila=/input,ro \
  --mount host=/mnt/xila/output,xila=/output,rw \
  -- /binaries/wasm /input/app.wasm
```

Bind mount permissions are checked by Linux as UID/GID `10001:10001`. Make
selected host directories readable or writable by that identity. Rootless
Docker and user namespace remapping translate container IDs; confirm the
resulting host ownership before choosing writable paths. The Xila user identity
is configured independently by the runner.

## Persist Xila system state

By default, `drive.img` lives in the container's writable layer and is removed
with the container. Use a named volume to retain Xila-owned disk state:

```sh
docker volume create xila-state
docker run --rm \
  --mount type=volume,src=xila-state,dst=/var/lib/xila \
  --mount type=bind,src="$PWD",dst=/mnt/xila/application,readonly \
  xila-runtime:local \
  run --disk /var/lib/xila/drive.img --seed /usr/share/xila/base.img --network=none \
  --mount host=/mnt/xila/application,xila=/application,ro \
  -- /binaries/wasm /application/app.wasm
```

The seed at `/usr/share/xila/base.img` is immutable. An existing working disk
is reused; the seed initializes the working disk only when it is absent. A
named volume mounted at `/var/lib/xila` must be writable by UID/GID 10001:10001.

## Stdio, networking, and read-only root

The runner is the container entrypoint, so its process receives signals and
application arguments directly. Standard input/output/error are connected to
the container streams. Allocate a TTY with `docker run -it` for interactive
programs. The default command disables Xila networking; the basic invocation
does not need `--privileged`, `NET_ADMIN`, or `/dev/net/tun`.

For a read-only image root, mount state and temporary storage explicitly:

```sh
docker run --rm --read-only \
  --mount type=volume,src=xila-state,dst=/var/lib/xila \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --mount type=bind,src="$PWD",dst=/mnt/xila/application,readonly \
  xila-runtime:local \
  run --disk /var/lib/xila/drive.img --seed /usr/share/xila/base.img --network=none \
  --mount host=/mnt/xila/application,xila=/application,ro \
  -- /binaries/wasm /application/app.wasm
```

Application exit status is returned by the runner and becomes the container
exit status. Runtime initialization and CLI errors return nonzero statuses.
The runner is PID 1, so Docker sends `SIGTERM` directly to it. The container
workflow checks that a graceful stop returns status 143 and that disk state is
still readable afterward. `SIGKILL` cannot flush pending state.

## Build status

The container workflow builds the `amd64` image and runs a WASI fixture under a
read-only root. It checks read-only and writable HostFS mounts, LittleFS state
persistence, guest exit-status propagation, and graceful signal handling. Pull
requests only build and test the image; pushes to `main` and `v*` tags publish it
to GitHub Container Registry after those checks pass.
