# Text Editor

Text Editor is a Xila WebAssembly application for creating and editing UTF-8 text files. It keeps a temporary working copy next to the selected file and reads or writes it through fixed-size buffers. The interface edits one UTF-8-safe page at a time, so memory use stays bounded as file size grows. The working copy needs free storage approximately equal to the source file and a writable parent directory.

Enter an absolute file path, then choose **New** or **Open**. Use **Previous** and **Next** to move through 4 KiB pages; **Save** streams the working copy to the selected path. Files containing invalid UTF-8 or NUL bytes are rejected. A failed save keeps the working copy available for retry.

Build and test the storage model with:

```sh
cargo test -p text_editor
cargo build -p text_editor --target wasm32-wasip1 --profile release-wasm
```
