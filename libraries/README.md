# Reusable libraries

A library provides reusable contracts, types, algorithms, adapters or concrete implementations without being installed as its own independently assembled runtime service.

A library may contain stateful types and may depend on a runtime subsystem where a contract genuinely requires it (for example executable launch helpers); directory placement is about architectural **ownership and lifecycle**, not a mechanically enforced acyclic dependency graph.

VFS device class contracts (`File::control` commands and ABI payloads) live in [devices](../devices/README.md), and generic VFS device implementations live in [drivers](../drivers/README.md). The final binary/examples assemble services, drivers and executables. See [subsystems](../subsystems/README.md).
