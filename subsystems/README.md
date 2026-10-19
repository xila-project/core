# Runtime subsystems

A subsystem owns or coordinates a service installed into an assembled Xila runtime. It manages lifecycle, state and/or process-wide integration such as the allocator, logger or LVGL. It may depend on reusable libraries and platform drivers.

Not every stateful Rust type is a subsystem: the defining criterion is **runtime ownership and assembly**, not complexity or the presence of mutable state.

Keep Cargo package names and public Rust APIs stable when reorganizing paths. See [libraries](../libraries/README.md) for shared contracts and implementations and the [architecture documentation](https://www.xila.dev/explanation/architecture/) for ownership details.
