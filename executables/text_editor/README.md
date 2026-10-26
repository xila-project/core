# Text Editor

Text Editor is a Xila WebAssembly application for creating and editing UTF-8 text files. It keeps a temporary working copy next to the selected file and reads or writes it through fixed-size buffers. The interface edits one UTF-8-safe page at a time, so memory use stays bounded as file size grows. The working copy needs free storage approximately equal to the source file and a writable parent directory.

Choose **Open** or **Save** to use the system LVGL file picker. **Folder** selects a directory and prepares a `new.txt` path there; choose **New** to create the document. The path field also accepts an absolute Xila VFS path. Use **Previous** and **Next** to move through 4 KiB pages; **Save** streams the working copy to the selected path. Cancelling a picker keeps the document and path unchanged. Files containing invalid UTF-8 or NUL bytes are rejected. A failed save keeps the working copy available for retry.

Build and test the storage model with:

```sh
cargo test -p xila-text-editor
cargo build -p xila-text-editor --target wasm32-wasip1 --profile release-wasm
```
