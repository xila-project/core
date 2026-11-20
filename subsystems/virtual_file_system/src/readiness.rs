use file_system::{AccessFlags, CharacterDevice, IoErrorKind, ReadyEvents};
use synchronization::Arc;

use crate::{ItemStatic, Result, SynchronousFile, pipe::Pipe};

/// A readiness probe detached from the I/O context. Retaining this probe does
/// not retain the opening or postpone pipe EOF. Character devices are static.
#[derive(Clone)]
pub struct FileReadiness {
    source: Source,
    access: AccessFlags,
}

#[derive(Clone)]
enum Source {
    Immediate,
    Pipe(Arc<Pipe>),
    Character(&'static dyn CharacterDevice),
    Invalid,
}

impl FileReadiness {
    pub fn is_pipe(&self) -> bool {
        matches!(self.source, Source::Pipe(_))
    }

    /// Return the events and whether they came from a precise readiness probe.
    pub fn events(&self, mut interests: ReadyEvents) -> Result<(ReadyEvents, bool)> {
        interests.set(
            ReadyEvents::READ,
            interests.contains(ReadyEvents::READ) && self.access.contains(AccessFlags::Read),
        );
        interests.set(
            ReadyEvents::WRITE,
            interests.contains(ReadyEvents::WRITE) && self.access.contains(AccessFlags::Write),
        );
        match &self.source {
            Source::Immediate => Ok((interests, true)),
            Source::Pipe(pipe) => Ok((pipe.readiness(self.access, interests), true)),
            Source::Character(device) => Ok(match device.readiness(interests)? {
                Some(events) => (events, true),
                None => (interests, false),
            }),
            Source::Invalid => Err(IoErrorKind::BadDescriptor),
        }
    }
}

impl SynchronousFile<'_> {
    pub fn readiness(&self) -> FileReadiness {
        let source = match &self.item {
            ItemStatic::File(_) | ItemStatic::BlockDevice(_) => Source::Immediate,
            ItemStatic::Pipe(pipe) => Source::Pipe(pipe.clone()),
            ItemStatic::CharacterDevice(device) => Source::Character(*device),
            _ => Source::Invalid,
        };
        FileReadiness {
            source,
            access: self.flags.get_access(),
        }
    }

    pub fn pipe_bytes_available(&self) -> Result<usize> {
        match &self.item {
            ItemStatic::Pipe(pipe) => Ok(pipe.bytes_available()),
            _ => Err(IoErrorKind::Unsupported),
        }
    }
}
