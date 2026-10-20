# Headless Xila runtime container

The runtime OCI image runs Xila applications and the native Xila CLI shell
without the LVGL desktop. The image is currently built for `linux/amd64` and
published as `ghcr.io/xila-project/xila-runtime:latest` after the container
workflow passes on `main`.

## Interactive shell

The image defaults to `xila-runtime shell --network=none`, so with Docker:

```sh
docker volume create xila-state
docker run --rm -it --network none \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --mount type=volume,src=xila-state,dst=/var/lib/xila \
  ghcr.io/xila-project/xila-runtime:latest
```

Or explicitly append `shell --network=none`. The shell is the **Xila CLI**,
not `/bin/sh` from Debian. It supports Xila built-ins such as `help`,
`pwd`, `ls`, `cd`, `echo`, and `exit`, and can execute mounted Xila
programs, including WASI modules via `/binaries/wasm`. Use `exit` or send
EOF to close a session.

On a new reference disk, login as `administrator` with an initially empty
password (press Enter). This is only a development default: set a password
before storing sensitive data. Login accounts persist in the LittleFS disk.
The shell mode loads the accounts from disk and rejects `--env User=...`
to avoid bypassing interactive authentication.

For additional host directories, **both** Docker and Xila VFS mounts are
required:

```sh
mkdir -p apps
docker run --rm -it --network none \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --mount type=volume,src=xila-state,dst=/var/lib/xila \
  --mount "type=bind,src=$PWD/apps,dst=/mnt/xila/apps,readonly" \
  ghcr.io/xila-project/xila-runtime:latest shell --network=none \
  --mount host=/mnt/xila/apps,xila=/apps,ro
```

Inside Xila, `/apps` exposes the host directory read-only. The runtime
does not implicitly share the container filesystem with Xila.

## One-shot WASI application

Explicit `run` mode is unchanged and requires an Xila executable:

```sh
docker run --rm -i --network none \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --mount type=volume,src=xila-state,dst=/var/lib/xila \
  --mount "type=bind,src=$PWD/apps,dst=/mnt/xila/apps,readonly" \
  ghcr.io/xila-project/xila-runtime:latest run --network=none \
  --mount host=/mnt/xila/apps,xila=/apps,ro \
  -- /binaries/wasm /apps/application.wasm
```

The image runs as UID/GID 10001 and uses `/var/lib/xila/drive.img` for
mutable LittleFS state. The seed is at `/usr/share/xila/base.img`.
Host networking remains disabled; do not grant the Docker container
network access expecting Xila guest networking to work.

## Build locally

```sh
docker build -f Dockerfile.runtime -t xila-runtime:local .
```

The CI workflow builds the image, checks one-shot execution and filesystem
persistence, and exercises an authenticated CLI session with piped stdin.
