# ADR-011: Reading Docker's disk use from its engine

- **Status:** Accepted
- **Context:**
  - The Developer page shows how much space Docker uses, broken down into images, build
    cache, volumes and containers.
  - Docker Desktop keeps everything in one disk image (`Docker.raw`), so its size alone
    doesn't say what is inside.
  - ADR-005 allows only read-only Docker inventory.
- **Decision:**
  - **While Docker is running**, Kleari connects to the engine's local Unix socket and sends
    one fixed request: `GET /system/df` over HTTP/1.0.
    - Sockets tried, in order: `~/.docker/run/docker.sock`, then `/var/run/docker.sock`.
    - That endpoint only reports usage. No other endpoint, method or body is ever sent, and the
      frontend can't choose any of it.
    - Limits: one 10 s deadline for the whole exchange across both sockets (volumes are
      measured), a 1 s write limit, and responses capped at 16 MiB. The exchange runs on the
      scan's thread with no helper thread: the socket connects non-blocking (polled in
      100 ms steps), and reads use 100 ms timeouts, so the deadline and cancellation are
      checked throughout and the exchange ends as soon as either is reached. A cancelled
      scan doesn't ask Docker at all and reports it as not checked.
    - Only a plain `200` HTTP/1.x response without chunked encoding is accepted. The JSON is
      parsed as untrusted input. A body with none of the expected fields is rejected.
    - Image layers are counted once (`LayersSize`). Shared build-cache records aren't counted
      twice. A missing section, or a volume or container whose size Docker didn't compute
      (`-1` or absent), marks the usage incomplete, and the page shows it as a minimum rather
      than as an exact zero.
  - **When it isn't running**, only the disk image's allocated size is shown, read without
    following symlinks, with "Start Docker and scan again".
  - **Kleari never runs a Docker command.** The page shows `docker system prune` for the
    user to run themselves.
- **Consequences:**
  - The breakdown needs Docker running. Kleari doesn't start it.
  - The socket is the user's own Docker engine. A process impersonating it could only feed
    numbers to one card, and the response is bounded and validated.
  - Any other Docker request would need its own decision, still within ADR-005.
  - `std` has no connect timeout for Unix sockets, so the connect is a small `libc`
    non-blocking connect. A pending connect (`EINPROGRESS`, `EINTR`) is polled, then
    `SO_ERROR` is read. A full backlog (`EAGAIN` on Linux; macOS refuses instead) is retried
    every 100 ms. An engine that never accepts costs at most the deadline and leaves nothing
    running.
  - The "is Docker installed" checks (`/Applications/Docker.app`, the Docker container
    folder), the disk image and the sockets are skipped under a non-local mount. The checks
    and the disk image are looked up through directory handles from `/` with no symlink on
    the way. The socket path itself is resolved by `connect`.
  - The mount table is a snapshot. The handle walk re-reads it before each component and
    after any step onto another volume. A network volume mounted in the instant between a
    check and the next lookup could still be reached once (by `connect` for a socket, or
    one `stat`/open for a folder) before the next check refuses it. Nothing inside such a
    volume is read.
