use core::{fmt, marker::PhantomData, ptr, str};

use crate::{checksum, crc32_step, write_marker, write_marker_raw};

pub const MAGIC: [u8; 4] = *b"XCR1";
pub const VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 256;
pub const ESPRESSIF_RECORD_CAPACITY: usize = 4096;
pub const NATIVE_RECORD_CAPACITY: usize = 16_384;

pub const MESSAGE_TRUNCATED: u16 = 1 << 0;
pub const STACK_TRUNCATED: u16 = 1 << 1;
pub const LOG_SKIPPED_CONTENTION: u16 = 1 << 2;
pub const FRAMES_TRUNCATED: u16 = 1 << 3;
pub const SECONDARY_CORE_UNHALTED: u16 = 1 << 4;

macro_rules! tag {
    ($name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum $name { $($variant = $value),+ }

        impl TryFrom<u8> for $name {
            type Error = DecodeError;

            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(DecodeError::InvalidField),
                }
            }
        }
    };
}

tag!(RecordKind {
    KernelPanic = 1,
    KernelException = 2,
    WatchdogReset = 3,
    AbnormalHardwareReset = 4,
    ApplicationCrash = 5,
});

tag!(Architecture {
    XtensaEsp32 = 1,
    XtensaEsp32S3 = 2,
    Riscv32 = 3,
    ArmThumbv7em = 4,
    NativeX86_64 = 5,
    NativeAarch64 = 6,
    Wasm32 = 7,
});

tag!(ResetReason {
    Unknown = 0,
    SoftwareReset = 1,
    Watchdog = 2,
    BrownOut = 3,
    CpuLockup = 4,
    PinReset = 5,
    PowerOn = 6,
    HostSignal = 7,
    HostPanic = 8,
});

tag!(BuildIdentifierKind {
    None = 0,
    ElfSha256 = 1,
    GnuBuildIdentifier = 2,
    WasmFingerprint = 3,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildIdentifier {
    None,
    ElfSha256([u8; 32]),
    GnuBuildIdentifier([u8; 20]),
    WasmFingerprint([u8; 32]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header<'a> {
    pub kind: RecordKind,
    pub architecture: Architecture,
    pub reset_reason: ResetReason,
    pub raw_reset_reason: u32,
    pub exception_code: u32,
    pub consecutive_crashes: u32,
    pub uptime_microseconds: u64,
    pub build_epoch: u64,
    pub program_counter: u64,
    pub stack_pointer: u64,
    pub link_register: u64,
    pub fault_address: u64,
    pub stack_base: u64,
    pub code_load_offset: u64,
    pub core_identifier: u32,
    pub task_identifier: u32,
    pub build_identifier: BuildIdentifier,
    pub task_name: &'a str,
    pub target_name: &'a str,
    pub firmware_version: &'a str,
    pub flags: u16,
}

impl Header<'_> {
    pub const fn new(kind: RecordKind, architecture: Architecture) -> Self {
        Self {
            kind,
            architecture,
            reset_reason: ResetReason::Unknown,
            raw_reset_reason: 0,
            exception_code: 0,
            consecutive_crashes: 0,
            uptime_microseconds: 0,
            build_epoch: 0,
            program_counter: 0,
            stack_pointer: 0,
            link_register: 0,
            fault_address: 0,
            stack_base: 0,
            code_load_offset: 0,
            core_identifier: 0,
            task_identifier: 0,
            build_identifier: BuildIdentifier::None,
            task_name: "",
            target_name: "",
            firmware_version: "",
            flags: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub frames: u8,
    pub message: u16,
    pub stack: u16,
    pub log_tail: u16,
}

impl Limits {
    pub const ESPRESSIF: Self = Self {
        frames: 24,
        message: 512,
        stack: 1024,
        log_tail: 2048,
    };
    pub const NATIVE: Self = Self {
        frames: 64,
        message: 2048,
        stack: 2048,
        log_tail: 10_240,
    };
}

/// A raw cursor into the record slot. Every access is a volatile read or
/// write through the raw pointer, so a cursor never creates a Rust reference
/// to the slot and never assumes exclusive access to it.
struct Cursor {
    pointer: *mut u8,
    length: usize,
}

impl Cursor {
    fn put(&mut self, offset: usize, value: &[u8]) {
        for (index, byte) in value.iter().enumerate() {
            // SAFETY: The writer constructors guarantee that
            // `pointer..pointer + length` is valid for reads and writes for
            // the lifetime of the writer.
            unsafe { ptr::write_volatile(self.pointer.add(offset + index), *byte) };
        }
    }

    fn fill(&mut self, offset: usize, length: usize, value: u8) {
        for index in 0..length {
            // SAFETY: Same range invariant as `put`.
            unsafe { ptr::write_volatile(self.pointer.add(offset + index), value) };
        }
    }

    /// CRC-32 over `start..end`, read back with volatile reads through the raw
    /// pointer. Reading the slot back (instead of trusting cached values)
    /// also covers bytes a concurrent writer may have changed, so a record
    /// torn by that writer fails the checksum on the next boot.
    fn checksum(&self, start: usize, end: usize) -> u32 {
        let mut checksum = u32::MAX;
        for offset in start..end {
            // SAFETY: Same range invariant as `put`.
            let byte = unsafe { ptr::read_volatile(self.pointer.add(offset)) };
            checksum = crc32_step(checksum, byte);
        }
        !checksum
    }

    /// Whether `offset..offset + length` is valid UTF-8, checked with volatile
    /// reads in small chunks so that no reference to the slot is created.
    fn is_utf8(&self, offset: usize, length: usize) -> bool {
        const CHUNK: usize = 64;
        let mut buffer = [0u8; CHUNK];
        let mut carried = 0usize;
        let mut position = 0usize;
        while position < length {
            let take = (length - position).min(CHUNK - carried);
            for index in 0..take {
                // SAFETY: Same range invariant as `put`.
                buffer[carried + index] =
                    unsafe { ptr::read_volatile(self.pointer.add(offset + position + index)) };
            }
            position += take;
            let filled = carried + take;
            match str::from_utf8(&buffer[..filled]) {
                Ok(_) => carried = 0,
                Err(error) if error.error_len().is_none() => {
                    // Incomplete sequence at the chunk end: carry its bytes.
                    let valid_up_to = error.valid_up_to();
                    buffer.copy_within(valid_up_to..filled, 0);
                    carried = filled - valid_up_to;
                    if carried >= 4 {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        carried == 0
    }
}

/// Shared writer implementation. All slot accesses go through a raw cursor
/// with volatile reads and writes, so the writer itself never creates a Rust
/// reference to the slot.
struct WriterCore {
    cursor: Cursor,
    limits: Limits,
    frame_count: u8,
    flags: u16,
}

impl WriterCore {
    /// Returns `None` if the slot cannot hold the fixed header.
    ///
    /// # Safety
    /// `cursor.pointer..cursor.pointer + cursor.length` must be valid for
    /// reads and writes for the lifetime of the returned writer.
    unsafe fn new(cursor: Cursor, header: &Header<'_>, limits: Limits) -> Option<Self> {
        if cursor.length < HEADER_SIZE {
            return None;
        }
        let mut this = Self {
            cursor,
            limits,
            frame_count: 0,
            flags: header.flags,
        };
        // SAFETY: The range invariant above holds for the header range.
        unsafe { write_marker_raw(this.cursor.pointer, [0; 4]) };
        this.cursor.fill(4, HEADER_SIZE - 4, 0);
        this.cursor.put(8, &VERSION.to_le_bytes());
        this.cursor.put(10, &(HEADER_SIZE as u16).to_le_bytes());
        this.cursor.put(16, &[header.kind as u8]);
        this.cursor.put(17, &[header.architecture as u8]);
        this.cursor.put(18, &[header.reset_reason as u8]);
        let (identifier_kind, identifier) = match &header.build_identifier {
            BuildIdentifier::None => (BuildIdentifierKind::None, [0; 32]),
            BuildIdentifier::ElfSha256(value) => (BuildIdentifierKind::ElfSha256, *value),
            BuildIdentifier::GnuBuildIdentifier(value) => {
                let mut identifier = [0; 32];
                identifier[..value.len()].copy_from_slice(value);
                (BuildIdentifierKind::GnuBuildIdentifier, identifier)
            }
            BuildIdentifier::WasmFingerprint(value) => {
                (BuildIdentifierKind::WasmFingerprint, *value)
            }
        };
        this.cursor.put(104, &identifier);
        this.cursor.put(19, &[identifier_kind as u8]);
        this.cursor.put(20, &header.raw_reset_reason.to_le_bytes());
        this.cursor.put(24, &header.exception_code.to_le_bytes());
        this.cursor
            .put(28, &header.consecutive_crashes.to_le_bytes());
        this.cursor
            .put(32, &header.uptime_microseconds.to_le_bytes());
        this.cursor.put(40, &header.build_epoch.to_le_bytes());
        this.cursor.put(48, &header.program_counter.to_le_bytes());
        this.cursor.put(56, &header.stack_pointer.to_le_bytes());
        this.cursor.put(64, &header.link_register.to_le_bytes());
        this.cursor.put(72, &header.fault_address.to_le_bytes());
        this.cursor.put(80, &header.stack_base.to_le_bytes());
        this.cursor.put(88, &header.code_load_offset.to_le_bytes());
        this.cursor.put(96, &header.core_identifier.to_le_bytes());
        this.cursor.put(100, &header.task_identifier.to_le_bytes());
        let task_name_length = header.task_name.floor_char_boundary(32);
        this.cursor
            .put(136, &header.task_name.as_bytes()[..task_name_length]);
        this.cursor.put(216, &[task_name_length as u8]);
        let target_name_length = header.target_name.floor_char_boundary(32);
        this.cursor
            .put(168, &header.target_name.as_bytes()[..target_name_length]);
        this.cursor.put(217, &[target_name_length as u8]);
        let version_length = header.firmware_version.floor_char_boundary(16);
        this.cursor
            .put(200, &header.firmware_version.as_bytes()[..version_length]);
        this.cursor.put(218, &[version_length as u8]);
        Some(this)
    }

    /// Returns false and sets `FRAMES_TRUNCATED` when the frame does not fit.
    fn push_frame(&mut self, address: u64) -> bool {
        let end = HEADER_SIZE + usize::from(self.frame_count) * 8;
        if self.frame_count == self.limits.frames || self.cursor.length - end < 8 {
            self.flags |= FRAMES_TRUNCATED;
            return false;
        }
        self.cursor.put(end, &address.to_le_bytes());
        self.frame_count += 1;
        true
    }

    /// Commits the record and returns its length in bytes. The callbacks
    /// receive the raw window into the slot.
    fn finish_with_stack(
        &mut self,
        message: fmt::Arguments<'_>,
        stack_length: usize,
        copy_stack: impl FnOnce(*mut u8, usize),
        snapshot_log: impl FnOnce(*mut u8, usize) -> Option<usize>,
    ) -> usize {
        use fmt::Write as _;

        let mut flags = self.flags;
        let mut end = HEADER_SIZE + usize::from(self.frame_count) * 8;
        let capacity = self.cursor.length - end;
        let message_capacity = capacity.min(usize::from(self.limits.message));
        let mut writer = MessageWriter {
            cursor: &mut self.cursor,
            base: end,
            capacity: message_capacity,
            length: 0,
            truncated: false,
        };
        if writer.write_fmt(message).is_err() || writer.truncated {
            flags |= MESSAGE_TRUNCATED;
        }
        let message_length = writer.length;
        end += message_length;

        let captured = stack_length
            .min(usize::from(self.limits.stack))
            .min(self.cursor.length - end);
        if captured < stack_length {
            flags |= STACK_TRUNCATED;
        }
        self.cursor.fill(end, captured, 0);
        // SAFETY: `end..end + captured` is within the slot range.
        copy_stack(unsafe { self.cursor.pointer.add(end) }, captured);
        end += captured;

        let log_capacity = (self.cursor.length - end).min(usize::from(self.limits.log_tail));
        // Initialize the buffer in case the callback reports bytes it did not write.
        self.cursor.fill(end, log_capacity, 0);
        // SAFETY: `end..end + log_capacity` is within the slot range.
        let log_length = match snapshot_log(unsafe { self.cursor.pointer.add(end) }, log_capacity) {
            Some(length) if length <= log_capacity && self.cursor.is_utf8(end, length) => length,
            Some(_) => 0,
            None => {
                flags |= LOG_SKIPPED_CONTENTION;
                0
            }
        };
        end += log_length;

        self.cursor.put(219, &[self.frame_count]);
        self.cursor.put(220, &(message_length as u16).to_le_bytes());
        self.cursor.put(222, &(captured as u16).to_le_bytes());
        self.cursor.put(224, &(log_length as u16).to_le_bytes());
        self.cursor.put(226, &flags.to_le_bytes());
        self.commit(end);
        end
    }

    fn commit(&mut self, end: usize) {
        self.cursor.put(12, &(end as u32).to_le_bytes());
        let checksum = self.cursor.checksum(8, end);
        self.cursor.put(4, &checksum.to_le_bytes());
        // SAFETY: The first 4 bytes are within the slot range; the magic goes
        // last, between fences.
        unsafe { write_marker_raw(self.cursor.pointer, MAGIC) };
    }
}

/// Writes the payload in place through an exclusive slot. Dropping this
/// writer before `finish` leaves the record invalid.
pub struct Writer<'a> {
    core: WriterCore,
    _exclusive: PhantomData<&'a mut [u8]>,
}

impl<'a> Writer<'a> {
    /// Returns `None` if the slot cannot hold the fixed header.
    pub fn new(output: &'a mut [u8], header: &Header<'_>, limits: Limits) -> Option<Self> {
        // SAFETY: `output` is an exclusive mutable borrow for `'a`, so the
        // slot range is valid and no other reference to it exists.
        unsafe {
            WriterCore::new(
                Cursor {
                    pointer: output.as_mut_ptr(),
                    length: output.len(),
                },
                header,
                limits,
            )
        }
        .map(|core| Self {
            core,
            _exclusive: PhantomData,
        })
    }

    /// Returns false and sets `FRAMES_TRUNCATED` when the frame does not fit.
    pub fn push_frame(&mut self, address: u64) -> bool {
        self.core.push_frame(address)
    }

    /// Commits the record and returns its valid bytes. The log callback copies
    /// a UTF-8 tail into the supplied buffer, or returns `None` on contention.
    pub fn finish(
        mut self,
        message: fmt::Arguments<'_>,
        stack: &[u8],
        snapshot_log: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> &'a [u8] {
        let pointer = self.core.cursor.pointer;
        let length = self.core.finish_with_stack(
            message,
            stack.len(),
            |window, length| {
                // SAFETY: The window is derived from the exclusive `&'a mut`.
                let output = unsafe { core::slice::from_raw_parts_mut(window, length) };
                output.copy_from_slice(&stack[..length])
            },
            |window, length| {
                // SAFETY: The window is derived from the exclusive `&'a mut`.
                let output = unsafe { core::slice::from_raw_parts_mut(window, length) };
                snapshot_log(output)
            },
        );
        // SAFETY: The slice is derived from the exclusive `&'a mut [u8]` passed
        // to `new` and is valid for `'a`.
        unsafe { core::slice::from_raw_parts(pointer, length) }
    }

    /// Like `finish`, but copies the stack directly into the output slot.
    /// The callback must fill the supplied window starting at `stack_base`.
    pub fn finish_with_stack(
        mut self,
        message: fmt::Arguments<'_>,
        stack_length: usize,
        copy_stack: impl FnOnce(&mut [u8]),
        snapshot_log: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> &'a [u8] {
        let pointer = self.core.cursor.pointer;
        let length = self.core.finish_with_stack(
            message,
            stack_length,
            |window, length| {
                // SAFETY: The window is derived from the exclusive `&'a mut`
                // passed to `new`.
                copy_stack(unsafe { core::slice::from_raw_parts_mut(window, length) })
            },
            |window, length| {
                // SAFETY: The window is derived from the exclusive `&'a mut`
                // passed to `new`.
                let output = unsafe { core::slice::from_raw_parts_mut(window, length) };
                snapshot_log(output)
            },
        );
        // SAFETY: The slice is derived from the exclusive `&'a mut [u8]` passed
        // to `new` and is valid for `'a`.
        unsafe { core::slice::from_raw_parts(pointer, length) }
    }
}

/// Raw writer for a slot the writer does not exclusively own, for example
/// the retained slot of a core that panicked first and still holds the
/// claim and may have a live reference to it: this writer never creates a
/// Rust reference to the slot. Every access goes through the raw cursor
/// with volatile reads and writes, and the magic is committed last between
/// fences, so a record torn by the concurrent writer fails the checksum on
/// the next boot.
pub struct RawWriter {
    core: WriterCore,
}

impl RawWriter {
    /// Returns `None` if the slot cannot hold the fixed header.
    ///
    /// # Safety
    /// The caller guarantees that `pointer..pointer.add(length)` is valid for
    /// reads and writes of `length` bytes for the lifetime of the returned
    /// writer (the range is byte-aligned). The writer never creates a Rust
    /// reference to that range. Concurrent unsynchronized access from other
    /// cores, including through the claim holder's reference, is the
    /// condition this type exists for and is tolerated: every access of the
    /// writer is volatile, and the record is validated by its checksum and
    /// the magic written last. A concurrent write that lands before the
    /// checksum read-back can produce a merged record that still validates
    /// (for example the header fields of one core with the stack bytes of
    /// the other); that is the accepted tradeoff of a single shared slot.
    pub unsafe fn new(
        pointer: *mut u8,
        length: usize,
        header: &Header<'_>,
        limits: Limits,
    ) -> Option<Self> {
        // SAFETY: The caller guarantees the range is valid for reads and
        // writes for the lifetime of the writer.
        unsafe { WriterCore::new(Cursor { pointer, length }, header, limits) }
            .map(|core| Self { core })
    }

    /// Returns false and sets `FRAMES_TRUNCATED` when the frame does not fit.
    pub fn push_frame(&mut self, address: u64) -> bool {
        self.core.push_frame(address)
    }

    /// Commits the record and returns its length in bytes. The callbacks
    /// receive the raw window into the slot and must not create Rust
    /// references to it.
    pub fn finish_with_stack(
        mut self,
        message: fmt::Arguments<'_>,
        stack_length: usize,
        copy_stack: impl FnOnce(*mut u8, usize),
        snapshot_log: impl FnOnce(*mut u8, usize) -> Option<usize>,
    ) -> usize {
        self.core
            .finish_with_stack(message, stack_length, copy_stack, snapshot_log)
    }
}

struct MessageWriter<'a> {
    cursor: &'a mut Cursor,
    base: usize,
    capacity: usize,
    length: usize,
    truncated: bool,
}

impl fmt::Write for MessageWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let remaining = self.capacity - self.length;
        let length = value.floor_char_boundary(remaining);
        self.cursor
            .put(self.base + self.length, &value.as_bytes()[..length]);
        self.length += length;
        if length < value.len() {
            self.truncated = true;
            return Err(fmt::Error);
        }
        Ok(())
    }
}

fn commit(record: &mut [u8]) {
    put(record, 12, (record.len() as u32).to_le_bytes());
    let checksum = checksum(&record[8..]);
    put(record, 4, checksum.to_le_bytes());
    write_marker(record, MAGIC);
}

/// Clear the retained magic. The caller must exclude other slot users.
pub fn invalidate(storage: &mut [u8; 4]) {
    write_marker(storage, [0; 4]);
}

/// Fill a missing reset reason and recompute the checksum of a valid record.
/// Returns the total record length, which can be smaller than the slot.
pub fn complete_reset_reason(
    bytes: &mut [u8],
    reason: ResetReason,
    raw_reason: u32,
) -> Result<usize, DecodeError> {
    let report = Record::read(bytes)?;
    let length = report.bytes().len();
    if report.header.reset_reason == ResetReason::Unknown {
        write_marker(bytes, [0; 4]);
        bytes[18] = reason as u8;
        put(bytes, 20, raw_reason.to_le_bytes());
        commit(&mut bytes[..length]);
    }
    Ok(length)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    InvalidMagic,
    UnsupportedVersion(u16),
    InvalidLength,
    InvalidChecksum,
    InvalidField,
    InvalidText,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Truncated => "Truncated crash record",
            Self::InvalidMagic => "Invalid crash record magic",
            Self::UnsupportedVersion(_) => "Unsupported crash record version",
            Self::InvalidLength => "Invalid crash record length",
            Self::InvalidChecksum => "Invalid crash record checksum",
            Self::InvalidField => "Invalid crash record field",
            Self::InvalidText => "Invalid UTF-8 in crash record",
        })
    }
}

impl core::error::Error for DecodeError {}

#[derive(Debug)]
pub struct Record<'a> {
    bytes: &'a [u8],
    frames: &'a [u8],
    pub header: Header<'a>,
    pub message: &'a str,
    pub stack_window: &'a [u8],
    pub log_tail: &'a str,
}

impl<'a> Record<'a> {
    /// Validate one record at the start of a retained slot or file buffer.
    pub fn read(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        let header = bytes.get(..HEADER_SIZE).ok_or(DecodeError::Truncated)?;
        if header[..4] != MAGIC {
            return Err(DecodeError::InvalidMagic);
        }
        let version = u16::from_le_bytes(get(header, 8));
        if version != VERSION {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        if usize::from(u16::from_le_bytes(get(header, 10))) != HEADER_SIZE {
            return Err(DecodeError::InvalidLength);
        }

        let frames_end = HEADER_SIZE + usize::from(header[219]) * 8;
        let message_end = frames_end + usize::from(u16::from_le_bytes(get(header, 220)));
        let stack_end = message_end + usize::from(u16::from_le_bytes(get(header, 222)));
        let log_end = stack_end + usize::from(u16::from_le_bytes(get(header, 224)));
        if u32::from_le_bytes(get(header, 12)) != log_end as u32 {
            return Err(DecodeError::InvalidLength);
        }
        let bytes = bytes.get(..log_end).ok_or(DecodeError::Truncated)?;
        if checksum(&bytes[8..]).to_le_bytes() != get::<4>(header, 4) {
            return Err(DecodeError::InvalidChecksum);
        }

        let build_identifier = match BuildIdentifierKind::try_from(header[19])? {
            BuildIdentifierKind::None => BuildIdentifier::None,
            BuildIdentifierKind::ElfSha256 => BuildIdentifier::ElfSha256(get(header, 104)),
            BuildIdentifierKind::GnuBuildIdentifier => {
                BuildIdentifier::GnuBuildIdentifier(get(header, 104))
            }
            BuildIdentifierKind::WasmFingerprint => {
                BuildIdentifier::WasmFingerprint(get(header, 104))
            }
        };
        Ok(Self {
            bytes,
            frames: &bytes[HEADER_SIZE..frames_end],
            header: Header {
                kind: RecordKind::try_from(header[16])?,
                architecture: Architecture::try_from(header[17])?,
                reset_reason: ResetReason::try_from(header[18])?,
                raw_reset_reason: u32::from_le_bytes(get(header, 20)),
                exception_code: u32::from_le_bytes(get(header, 24)),
                consecutive_crashes: u32::from_le_bytes(get(header, 28)),
                uptime_microseconds: u64::from_le_bytes(get(header, 32)),
                build_epoch: u64::from_le_bytes(get(header, 40)),
                program_counter: u64::from_le_bytes(get(header, 48)),
                stack_pointer: u64::from_le_bytes(get(header, 56)),
                link_register: u64::from_le_bytes(get(header, 64)),
                fault_address: u64::from_le_bytes(get(header, 72)),
                stack_base: u64::from_le_bytes(get(header, 80)),
                code_load_offset: u64::from_le_bytes(get(header, 88)),
                core_identifier: u32::from_le_bytes(get(header, 96)),
                task_identifier: u32::from_le_bytes(get(header, 100)),
                build_identifier,
                task_name: get_text(header, 136, 32, header[216])?,
                target_name: get_text(header, 168, 32, header[217])?,
                firmware_version: get_text(header, 200, 16, header[218])?,
                flags: u16::from_le_bytes(get(header, 226)),
            },
            message: text(&bytes[frames_end..message_end])?,
            stack_window: &bytes[message_end..stack_end],
            log_tail: text(&bytes[stack_end..])?,
        })
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn frames(&self) -> impl ExactSizeIterator<Item = u64> + 'a {
        self.frames
            .chunks_exact(8)
            .map(|chunk| u64::from_le_bytes(chunk.try_into().unwrap()))
    }
}

fn put<const SIZE: usize>(bytes: &mut [u8], offset: usize, value: [u8; SIZE]) {
    bytes[offset..offset + SIZE].copy_from_slice(&value);
}

fn get<const SIZE: usize>(bytes: &[u8], offset: usize) -> [u8; SIZE] {
    bytes[offset..offset + SIZE].try_into().unwrap()
}

fn get_text(bytes: &[u8], offset: usize, capacity: usize, length: u8) -> Result<&str, DecodeError> {
    let length = usize::from(length);
    if length > capacity {
        return Err(DecodeError::InvalidLength);
    }
    text(&bytes[offset..offset + length])
}

fn text(bytes: &[u8]) -> Result<&str, DecodeError> {
    str::from_utf8(bytes).map_err(|_| DecodeError::InvalidText)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header<'static> {
        Header {
            task_name: "Shell",
            target_name: "wt32-sc01-plus",
            firmware_version: "0.1.0",
            program_counter: 0x1234_5678,
            core_identifier: 1,
            task_identifier: 42,
            build_identifier: BuildIdentifier::ElfSha256([0xAB; 32]),
            ..Header::new(RecordKind::KernelPanic, Architecture::XtensaEsp32S3)
        }
    }

    #[test]
    fn round_trips_header_and_payload_at_fixed_offsets() {
        let mut slot = [0xFF; 512];
        let header = header();
        let mut writer = Writer::new(&mut slot, &header, Limits::ESPRESSIF).unwrap();
        assert!(writer.push_frame(0x0102_0304_0506_0708));
        let encoded = writer.finish(format_args!("Panic"), &[0xFF, 0xAA], |log| {
            log[..2].copy_from_slice(b"x\n");
            Some(2)
        });
        let length = encoded.len();
        assert_eq!(&encoded[..4], b"XCR1");
        assert_eq!(&encoded[8..12], &[1, 0, 0, 1]);
        assert_eq!(&encoded[48..56], &[0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0]);
        assert_eq!(&encoded[216..220], &[5, 14, 5, 1]);
        assert_eq!(&encoded[228..256], &[0; 28]);
        assert_eq!(&encoded[256..264], &[8, 7, 6, 5, 4, 3, 2, 1]);

        let record = Record::read(&slot).unwrap();
        assert_eq!(record.bytes().len(), length);
        assert_eq!(record.header, header);
        assert!(record.frames().eq([0x0102_0304_0506_0708]));
        assert_eq!(record.message, "Panic");
        assert_eq!(record.stack_window, [0xFF, 0xAA]);
        assert_eq!(record.log_tail, "x\n");
    }

    #[test]
    fn truncates_utf8_and_marks_limited_parts() {
        let mut slot = [0; HEADER_SIZE + 12];
        let limits = Limits {
            frames: 1,
            message: 2,
            stack: 1,
            log_tail: 0,
        };
        let mut writer = Writer::new(&mut slot, &header(), limits).unwrap();
        assert!(writer.push_frame(1));
        assert!(!writer.push_frame(2));
        let bytes = writer.finish(format_args!("aé"), &[1, 2], |_| None);
        let record = Record::read(bytes).unwrap();
        assert_eq!(record.message, "a");
        assert_eq!(record.stack_window, [1]);
        assert_eq!(record.log_tail, "");
        assert_eq!(
            record.header.flags,
            MESSAGE_TRUNCATED | STACK_TRUNCATED | FRAMES_TRUNCATED | LOG_SKIPPED_CONTENTION
        );
    }

    #[test]
    fn rejects_torn_truncated_and_corrupt_records() {
        let mut slot = [0; 512];
        Writer::new(&mut slot, &header(), Limits::ESPRESSIF).unwrap();
        assert_eq!(Record::read(&slot).unwrap_err(), DecodeError::InvalidMagic);
        let encoded = Writer::new(&mut slot, &header(), Limits::ESPRESSIF)
            .unwrap()
            .finish(format_args!("Panic"), &[], |_| Some(0));
        for length in 0..encoded.len() {
            assert_eq!(
                Record::read(&encoded[..length]).unwrap_err(),
                DecodeError::Truncated
            );
        }
        slot[HEADER_SIZE] ^= 1;
        assert_eq!(
            Record::read(&slot).unwrap_err(),
            DecodeError::InvalidChecksum
        );
        slot[12] ^= 1;
        assert_eq!(Record::read(&slot).unwrap_err(), DecodeError::InvalidLength);
        slot[8] = 2;
        assert_eq!(
            Record::read(&slot).unwrap_err(),
            DecodeError::UnsupportedVersion(2)
        );
    }

    #[test]
    fn raw_writer_produces_the_same_record_as_the_owned_writer() {
        let mut owned_slot = [0xFF; 512];
        let mut raw_slot = [0xFF; 512];
        let header = header();
        let mut writer = Writer::new(&mut owned_slot, &header, Limits::ESPRESSIF).unwrap();
        assert!(writer.push_frame(0x0102_0304_0506_0708));
        let owned = writer
            .finish(format_args!("Panic"), &[0xFF, 0xAA], |log| {
                log[..2].copy_from_slice(b"x\n");
                Some(2)
            })
            .len();

        // SAFETY: `raw_slot` is a local array that this test owns exclusively;
        // no reference to it is live while the raw writer runs.
        let mut raw = unsafe {
            RawWriter::new(
                raw_slot.as_mut_ptr(),
                raw_slot.len(),
                &header,
                Limits::ESPRESSIF,
            )
        }
        .unwrap();
        assert!(raw.push_frame(0x0102_0304_0506_0708));
        let length = raw.finish_with_stack(
            format_args!("Panic"),
            2,
            |pointer, length| {
                // SAFETY: The window is into `raw_slot`, owned by this test.
                let window = unsafe { core::slice::from_raw_parts_mut(pointer, length) };
                window.copy_from_slice(&[0xFF, 0xAA][..length]);
            },
            |pointer, length| {
                // SAFETY: The window is into `raw_slot`, owned by this test.
                let window = unsafe { core::slice::from_raw_parts_mut(pointer, length) };
                window[..2].copy_from_slice(b"x\n");
                Some(2)
            },
        );
        assert_eq!(length, owned);
        assert_eq!(&raw_slot[..length], &owned_slot[..owned]);
        let record = Record::read(&raw_slot).unwrap();
        assert_eq!(record.header, header);
        assert_eq!(record.message, "Panic");
        assert_eq!(record.stack_window, [0xFF, 0xAA]);
        assert_eq!(record.log_tail, "x\n");
    }

    #[test]
    fn raw_writer_rejects_a_slot_smaller_than_the_header() {
        let mut slot = [0; HEADER_SIZE - 1];
        // SAFETY: `slot` is a local array that this test owns exclusively.
        let raw =
            unsafe { RawWriter::new(slot.as_mut_ptr(), slot.len(), &header(), Limits::ESPRESSIF) };
        assert!(raw.is_none());
    }

    #[test]
    fn completes_reason_only_once() {
        let mut slot = [0; 512];
        Writer::new(&mut slot, &header(), Limits::ESPRESSIF)
            .unwrap()
            .finish(format_args!("Panic"), &[], |_| Some(0));
        complete_reset_reason(&mut slot, ResetReason::SoftwareReset, 4).unwrap();
        complete_reset_reason(&mut slot, ResetReason::Watchdog, 9).unwrap();
        let record = Record::read(&slot).unwrap();
        assert_eq!(record.header.reset_reason, ResetReason::SoftwareReset);
        assert_eq!(record.header.raw_reset_reason, 4);
    }
}
