use alloc::string::String;
use core::time::Duration;
use file_system::{AccessFlags, Path};
use task::TaskIdentifier;
use virtual_file_system::{File, VirtualFileSystem};

use crate::ExecutableContext;
use crate::Result;

pub struct Standard<'a> {
    pub context: &'static ExecutableContext,
    pub standard_in: File<'a>,
    pub standard_out: File<'a>,
    pub standard_error: File<'a>,
}

impl<'a> Standard<'a> {
    pub async fn open(
        standard_in: &impl AsRef<Path>,
        standard_out: &impl AsRef<Path>,
        standard_error: &impl AsRef<Path>,
        task: TaskIdentifier,
        virtual_file_system: &'a VirtualFileSystem,
        context: &'static ExecutableContext,
    ) -> Result<Self> {
        let standard_in = virtual_file_system
            .open(standard_in, AccessFlags::Read.into(), task)
            .await?;

        let standard_out = virtual_file_system
            .open(standard_out, AccessFlags::Write.into(), task)
            .await?;

        let standard_error = virtual_file_system
            .open(standard_error, AccessFlags::Write.into(), task)
            .await?;

        Ok(Self::new(
            context,
            standard_in,
            standard_out,
            standard_error,
        ))
    }

    pub fn new(
        context: &'static ExecutableContext,
        standard_in: File<'a>,
        standard_out: File<'a>,
        standard_error: File<'a>,
    ) -> Self {
        Self {
            context,
            standard_in,
            standard_out,
            standard_error,
        }
    }

    pub fn input<'s>(&'s mut self) -> &'s mut File<'a> {
        &mut self.standard_in
    }

    pub fn out<'s>(&'s mut self) -> &'s mut File<'a> {
        &mut self.standard_out
    }

    pub fn error<'s>(&'s mut self) -> &'s mut File<'a> {
        &mut self.standard_error
    }

    pub async fn read_line(&mut self, buffer: &mut String) -> virtual_file_system::Result<()> {
        buffer.clear();

        // This function read_until the source until the delimiter '\n' is found.
        // If the delimiter is not found, expand the buffer

        let mut temp_buffer = [0u8; 1];

        loop {
            let bytes_read = self.standard_in.read(&mut temp_buffer).await?;

            if bytes_read == 0 {
                task::sleep(Duration::from_millis(10)).await;
                continue;
            }

            if temp_buffer[0] == b'\n' {
                break;
            } else {
                buffer.push(temp_buffer[0] as char);
            }
        }

        Ok(())
    }

    pub async fn duplicate(&self) -> virtual_file_system::Result<Self> {
        Ok(Self {
            context: self.context,
            standard_in: self.standard_in.duplicate().await?,
            standard_out: self.standard_out.duplicate().await?,
            standard_error: self.standard_error.duplicate().await?,
        })
    }

    pub fn split(self) -> (File<'a>, File<'a>, File<'a>) {
        (self.standard_in, self.standard_out, self.standard_error)
    }

    pub async fn close(
        self,
        virtual_file_system: &VirtualFileSystem,
    ) -> virtual_file_system::Result<()> {
        self.standard_in.close(virtual_file_system).await?;
        self.standard_out.close(virtual_file_system).await?;
        self.standard_error.close(virtual_file_system).await?;

        Ok(())
    }
}
