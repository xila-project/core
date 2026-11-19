# Xila façade

`xila` is the public OS façade. The repository root is a virtual Cargo
workspace and selects this package by default; other crates can use the
workspace dependency `xila` or depend directly on smaller contracts. The façade,
main-workspace libraries and subsystems inherit their version from the root;
applications, drivers and standalone workspaces keep their own versions.

The services re-exported unconditionally are available with
`--no-default-features`. ABI definitions are enabled by default and can be
selected with `abi_definitions`; audio, bootsplash and graphics remain opt-in.
`graphics` forwards to the executable's graphics support, and the
`graphics_rendering_rgb565` and `graphics_rendering_xrgb8888` features select
the corresponding pixel format. The baseline still includes networking through
`executable`; disabling graphics does not make the full façade a minimal MCU
runtime. Use the individual libraries for smaller configurations.

The minimum supported Rust version is 1.97. Development and CI use Rust
1.97.0. Host checks do not establish support on any particular MCU.
