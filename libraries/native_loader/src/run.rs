extern crate alloc;

use alloc::{string::String, vec::Vec};

use abi_application::{Directory, File, XilaStartup, run_main};

use crate::LoadedImage;

/// Capabilities and arguments owned by the loaded program's coroutine.
#[derive(Default)]
pub struct Startup {
    pub arguments: Vec<String>,
    pub standard_input: Option<File>,
    pub standard_output: Option<File>,
    pub standard_error: Option<File>,
    pub home: Option<Directory>,
}

impl LoadedImage {
    /// Run the ELF entry point on the calling task's executor.
    ///
    /// # Safety
    /// The image must come from a trusted source and implement
    /// `int32_t _xila_start(const XilaStartup *)`. Its entry and callees must
    /// obey the startup lifetime rules, fit on the stack, and return without
    /// unwinding or leaving code or data references in other tasks. The caller
    /// must meet the executor affinity and single-core requirements of
    /// `stackful::spawn`.
    pub async unsafe fn spawn<T: 'static>(
        self,
        manager: &'static task::Manager,
        parent: task::TaskIdentifier,
        name: &str,
        stack: stackful::Stack,
        startup: Startup,
        map: impl FnOnce(i32) -> T + 'static,
    ) -> task::Result<task::JoinHandle<T>> {
        unsafe {
            stackful::spawn_with_result(
                manager,
                parent,
                name,
                stack,
                move || self.run(startup),
                map,
            )
            .await
            .map(|(handle, _)| handle)
        }
    }

    unsafe fn run(self, startup: Startup) -> i32 {
        let address = self.entry_address().as_ptr();
        let entry: unsafe extern "C" fn(*const XilaStartup) -> i32 =
            unsafe { core::mem::transmute(address) };
        let args: Vec<&str> = startup.arguments.iter().map(String::as_str).collect();
        // M1 supplies the API table through the startup constructed by run_main.
        unsafe {
            run_main(
                entry,
                &args,
                startup.standard_input,
                startup.standard_output,
                startup.standard_error,
                startup.home,
            )
        }
    }
}
