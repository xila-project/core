This copy of `littlefs2-sys` is based on Xila's upstream fork at
`2c2aab090f7592d79eec991656c03797d1a22c8f`. It contains the source files
needed for both upstream LittleFS variants and their BSD licenses. Its build
script defaults to Core's `libraries/little_fs/include/little_fs_config.h`,
while retaining the `LITTLEFS_CONFIG` override for downstream targets.
