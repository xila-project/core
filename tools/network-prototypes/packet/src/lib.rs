//! Standalone, host-only experiment: passt's QEMU stream transport as a smoltcp device.
use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
};

pub const FRAME_MAX: usize = 1514;
pub const QUEUE_CAPACITY: usize = 8;

struct Frame {
    bytes: [u8; FRAME_MAX + 4],
    len: usize,
}
impl Frame {
    fn empty() -> Self {
        Self {
            bytes: [0; FRAME_MAX + 4],
            len: 0,
        }
    }
}

/// The transport must be nonblocking. Storage and each pump's work are bounded.
pub struct PacketDevice<T> {
    io: T,
    rx: VecDeque<Frame>,
    tx: VecDeque<Frame>,
    incoming: Frame,
    read_offset: usize,
    write_offset: usize,
    failure: Option<io::ErrorKind>,
    orderly_eof: bool,
}
impl<T: Read + Write> PacketDevice<T> {
    pub fn new(io: T) -> Self {
        Self {
            io,
            rx: VecDeque::with_capacity(QUEUE_CAPACITY),
            tx: VecDeque::with_capacity(QUEUE_CAPACITY),
            incoming: Frame::empty(),
            read_offset: 0,
            write_offset: 0,
            failure: None,
            orderly_eof: false,
        }
    }
    pub fn error(&self) -> Option<io::ErrorKind> {
        self.failure
    }
    /// Inline device plus allocated queue slots; excludes transport kernel buffers.
    pub fn storage_bytes(&self) -> usize {
        std::mem::size_of_val(self)
            + (self.rx.capacity() + self.tx.capacity()) * std::mem::size_of::<Frame>()
    }
    pub fn pump(&mut self) -> io::Result<()> {
        if let Some(kind) = self.failure {
            return Err(kind.into());
        }
        let result = self.pump_inner();
        if let Err(ref error) = result {
            self.failure = Some(error.kind());
        }
        result
    }
    fn pump_inner(&mut self) -> io::Result<()> {
        // Separate budgets prevent a busy peer starving outgoing packets.
        for _ in 0..64 {
            if self.rx.len() == QUEUE_CAPACITY {
                break;
            }
            let target = if self.read_offset < 4 {
                4
            } else {
                self.incoming.len
            };
            match self
                .io
                .read(&mut self.incoming.bytes[self.read_offset..target])
            {
                Ok(0) => {
                    self.orderly_eof = self.read_offset == 0;
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                Ok(n) => {
                    self.read_offset += n;
                    if self.read_offset == 4 {
                        let len = u32::from_be_bytes(self.incoming.bytes[..4].try_into().unwrap())
                            as usize;
                        if !(14..=FRAME_MAX).contains(&len) {
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                        self.incoming.len = len + 4;
                    }
                    if self.read_offset == self.incoming.len {
                        self.rx
                            .push_back(std::mem::replace(&mut self.incoming, Frame::empty()));
                        self.read_offset = 0;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        for _ in 0..64 {
            let Some(frame) = self.tx.front() else {
                break;
            };
            match self.io.write(&frame.bytes[self.write_offset..frame.len]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.write_offset += n;
                    if self.write_offset == frame.len {
                        self.tx.pop_front();
                        self.write_offset = 0;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    pub fn pop_frame(&mut self) -> Option<Vec<u8>> {
        self.rx.pop_front().map(|f| f.bytes[4..f.len].to_vec())
    }
    pub fn queue_frame(&mut self, bytes: &[u8]) -> io::Result<()> {
        if !(14..=FRAME_MAX).contains(&bytes.len()) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if let Some(kind) = self.failure {
            return Err(kind.into());
        }
        if self.tx.len() == QUEUE_CAPACITY {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let mut frame = Frame::empty();
        frame.len = bytes.len() + 4;
        frame.bytes[..4].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        frame.bytes[4..frame.len].copy_from_slice(bytes);
        self.tx.push_back(frame);
        Ok(())
    }
}

pub struct ReceiveFrame(Frame);
impl RxToken for ReceiveFrame {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0.bytes[4..self.0.len])
    }
}
// None is the discard-only reply token required to deliver accepted frames
// after orderly EOF. It never enqueues or attempts I/O on the closed transport.
pub struct TransmitFrame<'a>(Option<&'a mut VecDeque<Frame>>);
impl TxToken for TransmitFrame<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        assert!(
            (14..=FRAME_MAX).contains(&len),
            "smoltcp frame exceeds negotiated MTU"
        );
        let mut frame = Frame::empty();
        frame.len = len + 4;
        frame.bytes[..4].copy_from_slice(&(len as u32).to_be_bytes());
        let result = f(&mut frame.bytes[4..frame.len]);
        if let Some(queue) = self.0 {
            queue.push_back(frame);
        }
        result
    }
}
impl<T: Read + Write> Device for PacketDevice<T> {
    type RxToken<'a>
        = ReceiveFrame
    where
        Self: 'a;
    type TxToken<'a>
        = TransmitFrame<'a>
    where
        Self: 'a;
    fn receive(&mut self, _: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        // smoltcp's regular interface polling drives I/O. Its Device trait has
        // no error channel; callers/controllers can inspect the latched error.
        let _ = self.pump();
        if self.orderly_eof {
            return self
                .rx
                .pop_front()
                .map(|frame| (ReceiveFrame(frame), TransmitFrame(None)));
        }
        if self.failure.is_some() || self.tx.len() == QUEUE_CAPACITY {
            return None;
        }
        self.rx
            .pop_front()
            .map(|frame| (ReceiveFrame(frame), TransmitFrame(Some(&mut self.tx))))
    }
    fn transmit(&mut self, _: Instant) -> Option<Self::TxToken<'_>> {
        if self.failure.is_some() || self.tx.len() == QUEUE_CAPACITY {
            None
        } else {
            Some(TransmitFrame(Some(&mut self.tx)))
        }
    }
    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = FRAME_MAX;
        caps.max_burst_size = Some(QUEUE_CAPACITY);
        caps
    }
}
