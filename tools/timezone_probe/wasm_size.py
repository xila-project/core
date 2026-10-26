#!/usr/bin/env python3
"""Report sections and declared memory for the memory32 modules this probe emits.

Initial memory includes Rust's reserved stack; it is not live heap/RAM usage.
Section payload sizes include their encoding overhead, not just instructions/data.
"""
import json
import pathlib
import sys


def uleb(data, position):
    value = 0
    for shift in range(0, 35, 7):
        byte = data[position]
        position += 1
        value |= (byte & 127) << shift
        if byte < 128:
            return value, position
    raise ValueError("invalid memory32 LEB128")


def measure(path):
    data = pathlib.Path(path).read_bytes()
    if data[:8] != b"\x00asm\x01\x00\x00\x00":
        raise ValueError("expected a WebAssembly version 1 module")
    position = 8
    sections = {}
    memory = []
    while position < len(data):
        section = data[position]
        length, payload = uleb(data, position + 1)
        end = payload + length
        if end > len(data):
            raise ValueError("section extends past module")
        sections[section] = sections.get(section, 0) + length
        if section == 5:
            count, cursor = uleb(data, payload)
            for _ in range(count):
                flags, cursor = uleb(data, cursor)
                if flags not in (0, 1):
                    raise ValueError("probe expects unshared memory32")
                minimum, cursor = uleb(data, cursor)
                memory.append(minimum * 65536)
                if flags == 1:
                    _, cursor = uleb(data, cursor)
        position = end
    return {"module_bytes": len(data), "code_section_bytes": sections.get(10, 0),
            "data_section_bytes": sections.get(11, 0), "initial_memory_bytes": memory}


if __name__ == "__main__":
    print(json.dumps(measure(sys.argv[1]), sort_keys=True))
