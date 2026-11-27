use core::{fmt, str};

use crate::{checksum, write_marker};

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

/// Writes the payload in place. Dropping this writer before `finish` leaves
/// the record invalid.
pub struct Writer<'a> {
    output: &'a mut [u8],
    limits: Limits,
    frame_count: u8,
    flags: u16,
}

impl<'a> Writer<'a> {
    /// Returns `None` if the slot cannot hold the fixed header.
    pub fn new(output: &'a mut [u8], header: &Header<'_>, limits: Limits) -> Option<Self> {
        let bytes = output.get_mut(..HEADER_SIZE)?;
        write_marker(bytes, [0; 4]);
        bytes[4..].fill(0);
        put(bytes, 8, VERSION.to_le_bytes());
        put(bytes, 10, (HEADER_SIZE as u16).to_le_bytes());
        bytes[16] = header.kind as u8;
        bytes[17] = header.architecture as u8;
        bytes[18] = header.reset_reason as u8;
        let identifier = &mut bytes[104..136];
        let identifier_kind = match &header.build_identifier {
            BuildIdentifier::None => BuildIdentifierKind::None,
            BuildIdentifier::ElfSha256(value) => {
                identifier.copy_from_slice(value);
                BuildIdentifierKind::ElfSha256
            }
            BuildIdentifier::GnuBuildIdentifier(value) => {
                identifier[..value.len()].copy_from_slice(value);
                BuildIdentifierKind::GnuBuildIdentifier
            }
            BuildIdentifier::WasmFingerprint(value) => {
                identifier.copy_from_slice(value);
                BuildIdentifierKind::WasmFingerprint
            }
        };
        bytes[19] = identifier_kind as u8;
        put(bytes, 20, header.raw_reset_reason.to_le_bytes());
        put(bytes, 24, header.exception_code.to_le_bytes());
        put(bytes, 28, header.consecutive_crashes.to_le_bytes());
        put(bytes, 32, header.uptime_microseconds.to_le_bytes());
        put(bytes, 40, header.build_epoch.to_le_bytes());
        put(bytes, 48, header.program_counter.to_le_bytes());
        put(bytes, 56, header.stack_pointer.to_le_bytes());
        put(bytes, 64, header.link_register.to_le_bytes());
        put(bytes, 72, header.fault_address.to_le_bytes());
        put(bytes, 80, header.stack_base.to_le_bytes());
        put(bytes, 88, header.code_load_offset.to_le_bytes());
        put(bytes, 96, header.core_identifier.to_le_bytes());
        put(bytes, 100, header.task_identifier.to_le_bytes());
        bytes[216] = put_text(bytes, 136, 32, header.task_name);
        bytes[217] = put_text(bytes, 168, 32, header.target_name);
        bytes[218] = put_text(bytes, 200, 16, header.firmware_version);
        Some(Self {
            output,
            limits,
            frame_count: 0,
            flags: header.flags,
        })
    }

    /// Returns false and sets `FRAMES_TRUNCATED` when the frame does not fit.
    pub fn push_frame(&mut self, address: u64) -> bool {
        let end = HEADER_SIZE + usize::from(self.frame_count) * 8;
        if self.frame_count == self.limits.frames || self.output.len() - end < 8 {
            self.flags |= FRAMES_TRUNCATED;
            return false;
        }
        put(self.output, end, address.to_le_bytes());
        self.frame_count += 1;
        true
    }

    /// Commits the record and returns its valid bytes. The log callback copies
    /// a UTF-8 tail into the supplied buffer, or returns `None` on contention.
    pub fn finish(
        self,
        message: fmt::Arguments<'_>,
        stack: &[u8],
        snapshot_log: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> &'a [u8] {
        use fmt::Write as _;

        let mut flags = self.flags;
        let mut end = HEADER_SIZE + usize::from(self.frame_count) * 8;
        let capacity = self.output.len() - end;
        let message_capacity = capacity.min(usize::from(self.limits.message));
        let mut writer = MessageWriter {
            buffer: &mut self.output[end..end + message_capacity],
            length: 0,
            truncated: false,
        };
        if writer.write_fmt(message).is_err() || writer.truncated {
            flags |= MESSAGE_TRUNCATED;
        }
        let message_length = writer.length;
        end += message_length;

        let stack_length = stack
            .len()
            .min(usize::from(self.limits.stack))
            .min(self.output.len() - end);
        if stack_length < stack.len() {
            flags |= STACK_TRUNCATED;
        }
        self.output[end..end + stack_length].copy_from_slice(&stack[..stack_length]);
        end += stack_length;

        let log_capacity = (self.output.len() - end).min(usize::from(self.limits.log_tail));
        let log = &mut self.output[end..end + log_capacity];
        // Initialize the buffer in case the callback reports bytes it did not write.
        log.fill(0);
        let log_length = match snapshot_log(log) {
            Some(length) if length <= log.len() && str::from_utf8(&log[..length]).is_ok() => length,
            Some(_) => 0,
            None => {
                flags |= LOG_SKIPPED_CONTENTION;
                0
            }
        };
        end += log_length;

        self.output[219] = self.frame_count;
        put(self.output, 220, (message_length as u16).to_le_bytes());
        put(self.output, 222, (stack_length as u16).to_le_bytes());
        put(self.output, 224, (log_length as u16).to_le_bytes());
        put(self.output, 226, flags.to_le_bytes());
        commit(&mut self.output[..end]);
        &self.output[..end]
    }
}

struct MessageWriter<'a> {
    buffer: &'a mut [u8],
    length: usize,
    truncated: bool,
}

impl fmt::Write for MessageWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let remaining = self.buffer.len() - self.length;
        let length = value.floor_char_boundary(remaining);
        self.buffer[self.length..self.length + length].copy_from_slice(&value.as_bytes()[..length]);
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

fn put_text(bytes: &mut [u8], offset: usize, capacity: usize, value: &str) -> u8 {
    let length = value.floor_char_boundary(capacity);
    bytes[offset..offset + length].copy_from_slice(&value.as_bytes()[..length]);
    length as u8
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
