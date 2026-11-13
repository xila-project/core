# Drivers

Xila splits every VFS device class into four layers, following the Linux and NuttX
upper-half / lower-half driver model:

| Layer | Role | Location | Linux / NuttX equivalent |
| --- | --- | --- | --- |
| Contract | Kind byte, `define_command!` identifiers and versioned `#[repr(C)]` payloads seen by applications through `File::control` | [`devices/`](../devices/README.md) (`device::<class>`, kind in `device::kind`) | `include/uapi/linux/*.h`, `ioctl-number.rst` / `include/nuttx/<class>/*.h` |
| Lower-half interface | Hardware-facing trait a target implements (`peripherals::gpio::GpioController`, a backend trait, …) | [`libraries/`](../libraries/README.md), or the owning subsystem | `struct gpio_chip` / lower-half `ops` structure |
| Upper half | Generic, platform-independent VFS device adapting the lower-half trait to the contract | `drivers/<class>`, **unless** the class is backed by a runtime service (see below) | `drivers/<class>/` core (`gpiolib`, `i2c-core`) / `drivers/<class>/` upper half |
| Lower-half implementation | Code tied to a target: SDK, flash layout, entropy source, accelerator | Platforms repository | `drivers/gpio/gpio-*.c`, `arch/` / `arch/`, `boards/` |

## Where an upper half lives

- **Class backed by a runtime service** (network stack, graphics/LVGL, audio mixer): the
  owning subsystem provides the device nodes, as Linux does in `net/`, `sound/` and
  `drivers/gpu/drm`.
- **Otherwise**: `drivers/<class>`, one crate per class or family (`drivers/peripherals` for
  GPIO, I2C, SPI, UART, ADC, PWM and system power).

Platform-independent lower-half implementations that need no target choice (null device,
deterministic mocks, pure-software fallbacks) also live in `drivers/`; anything that selects
a target SDK, peripheral or accelerator belongs to Platforms.

A new device class therefore adds its contract to `devices/` and registers its kind byte in
`device::kind` first, then its upper half here (or in its subsystem), and its target
implementations in Platforms.
