use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use xila_packet_network_prototype::{FRAME_MAX, PacketDevice, QUEUE_CAPACITY};

// Short operations force fragmentation without depending on kernel buffer size.
struct ShortIo(UnixStream, usize);
impl Read for ShortIo {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let n = b.len().min(3);
        self.0.read(&mut b[..n])
    }
}
impl Write for ShortIo {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.1 += 1;
        if self.1.is_multiple_of(2) {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.0.write(&b[..b.len().min(3)])
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn pair() -> (PacketDevice<ShortIo>, UnixStream) {
    let (a, b) = UnixStream::pair().unwrap();
    a.set_nonblocking(true).unwrap();
    b.set_nonblocking(true).unwrap();
    (PacketDevice::new(ShortIo(a, 0)), b)
}

#[test]
fn receives_fragmented_headers_and_coalesced_frames() {
    let (mut d, mut peer) = pair();
    peer.write_all(&[0, 0]).unwrap();
    d.pump().unwrap();
    assert!(d.pop_frame().is_none());
    peer.write_all(&[0, 14]).unwrap();
    peer.write_all(&[0x55; 14]).unwrap();
    peer.write_all(&[0, 0, 0, 14]).unwrap();
    peer.write_all(&[0xaa; 14]).unwrap();
    for _ in 0..10 {
        d.pump().unwrap();
    }
    assert_eq!(d.pop_frame().unwrap(), [0x55; 14]);
    assert_eq!(d.pop_frame().unwrap(), [0xaa; 14]);
    assert!(d.pop_frame().is_none());
}

#[test]
fn sends_big_endian_length_and_resumes_short_writes() {
    let (mut d, mut peer) = pair();
    d.queue_frame(&[0xaa; 14]).unwrap();
    for _ in 0..10 {
        d.pump().unwrap();
    }
    let mut got = [0; 18];
    peer.read_exact(&mut got).unwrap();
    assert_eq!(&got[..4], &[0, 0, 0, 14]);
    assert_eq!(&got[4..], &[0xaa; 14]);
}

#[test]
fn rejects_malformed_lengths_before_payload_allocation() {
    for len in [0u32, 13, FRAME_MAX as u32 + 1, u32::MAX] {
        let (mut d, mut peer) = pair();
        peer.write_all(&len.to_be_bytes()).unwrap();
        assert_eq!(d.pump().unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(d.pump().is_err(), "protocol errors must be terminal");
    }
}

#[test]
fn disconnect_and_truncated_frame_are_reported() {
    for bytes in [&[][..], &[0, 0][..], &[0, 0, 0, 14, 1][..]] {
        let (mut d, mut peer) = pair();
        peer.write_all(bytes).unwrap();
        drop(peer);
        assert!(d.pump().is_err());
        assert!(d.pump().is_err());
    }
}

#[test]
fn transmit_queue_is_bounded_and_validates_frames() {
    let (mut d, _peer) = pair();
    assert_eq!(
        d.queue_frame(&[0; 13]).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(d.queue_frame(&vec![0; FRAME_MAX + 1]).is_err());
    for _ in 0..QUEUE_CAPACITY {
        d.queue_frame(&[0; 14]).unwrap();
    }
    assert_eq!(
        d.queue_frame(&[0; 14]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn receive_queue_applies_backpressure_without_losing_frames() {
    let (mut d, mut peer) = pair();
    for i in 0..QUEUE_CAPACITY + 2 {
        peer.write_all(&[0, 0, 0, 14]).unwrap();
        peer.write_all(&[i as u8; 14]).unwrap();
    }
    for _ in 0..20 {
        d.pump().unwrap();
    }
    for i in 0..QUEUE_CAPACITY {
        assert_eq!(d.pop_frame().unwrap(), [i as u8; 14]);
    }
    assert!(d.pop_frame().is_none());
    for _ in 0..10 {
        d.pump().unwrap();
    }
    for i in QUEUE_CAPACITY..QUEUE_CAPACITY + 2 {
        assert_eq!(d.pop_frame().unwrap(), [i as u8; 14]);
    }
}

#[test]
fn smoltcp_tokens_deliver_ethernet_frames_and_apply_transmit_backpressure() {
    use smoltcp::{
        phy::{Device, Medium, RxToken, TxToken},
        time::Instant,
    };
    let (mut d, mut peer) = pair();
    peer.write_all(&[0, 0, 0, 14]).unwrap();
    peer.write_all(&[0x55; 14]).unwrap();
    d.pump().unwrap();
    let (rx, tx) = d.receive(Instant::from_millis(0)).unwrap();
    rx.consume(|frame| assert_eq!(frame, &[0x55; 14]));
    tx.consume(14, |frame| frame.fill(0xaa));
    for _ in 1..QUEUE_CAPACITY {
        d.transmit(Instant::from_millis(0))
            .unwrap()
            .consume(14, |frame| frame.fill(0xaa));
    }
    assert!(d.transmit(Instant::from_millis(0)).is_none());
    assert_eq!(d.capabilities().medium, Medium::Ethernet);
    for _ in 0..100 {
        d.pump().unwrap();
    }
    let mut got = [0; 18];
    peer.read_exact(&mut got).unwrap();
    assert_eq!(&got[..4], &[0, 0, 0, 14]);
    assert_eq!(&got[4..], &[0xaa; 14]);
    assert!(d.transmit(Instant::from_millis(0)).is_some());
}

#[test]
fn disconnect_disables_smoltcp_tokens_and_new_packets() {
    use smoltcp::{phy::Device, time::Instant};
    let (mut d, peer) = pair();
    drop(peer);
    assert!(d.pump().is_err());
    assert!(d.transmit(Instant::from_millis(0)).is_none());
    assert!(d.receive(Instant::from_millis(0)).is_none());
    assert!(d.queue_frame(&[0; 14]).is_err());
}

#[test]
fn malformed_length_after_valid_frame_terminates_entire_stream() {
    use smoltcp::{phy::Device, time::Instant};
    let (mut d, mut peer) = pair();
    peer.write_all(&[0, 0, 0, 14]).unwrap();
    peer.write_all(&[0x55; 14]).unwrap();
    peer.write_all(&[0xff; 64]).unwrap();
    assert_eq!(d.pump().unwrap_err().kind(), io::ErrorKind::InvalidData);
    assert!(d.receive(Instant::from_millis(0)).is_none());
    assert!(d.transmit(Instant::from_millis(0)).is_none());
}

#[test]
fn orderly_eof_drains_final_frame_with_discard_only_reply_token() {
    use smoltcp::{
        phy::{Device, RxToken, TxToken},
        time::Instant,
    };
    let (mut d, mut peer) = pair();
    peer.write_all(&[0, 0, 0, 14]).unwrap();
    peer.write_all(&[0x55; 14]).unwrap();
    drop(peer);
    let (rx, reply) = d
        .receive(Instant::from_millis(0))
        .expect("deliver accepted final frame");
    rx.consume(|frame| assert_eq!(frame, &[0x55; 14]));
    reply.consume(14, |frame| frame.fill(0xaa));
    assert!(d.error().is_some());
    assert!(d.transmit(Instant::from_millis(0)).is_none());
    assert!(d.receive(Instant::from_millis(0)).is_none());
    assert!(d.queue_frame(&[0; 14]).is_err());
}

#[test]
fn orderly_eof_drains_with_full_tx_queue_without_growing_it() {
    use smoltcp::{
        phy::{Device, RxToken, TxToken},
        time::Instant,
    };
    let (mut d, mut peer) = pair();
    let storage = d.storage_bytes();
    for _ in 0..QUEUE_CAPACITY {
        d.queue_frame(&[0; 14]).unwrap();
    }
    for i in 0..2 {
        peer.write_all(&[0, 0, 0, 14]).unwrap();
        peer.write_all(&[i; 14]).unwrap();
    }
    drop(peer);
    for i in 0..2 {
        let (rx, reply) = d
            .receive(Instant::from_millis(0))
            .expect("drain despite full TX queue");
        rx.consume(|frame| assert_eq!(frame, &[i; 14]));
        reply.consume(14, |frame| frame.fill(0xaa));
    }
    assert!(d.receive(Instant::from_millis(0)).is_none());
    assert!(d.transmit(Instant::from_millis(0)).is_none());
    assert_eq!(d.storage_bytes(), storage);
}

#[test]
fn truncated_frame_after_complete_frame_remains_fail_closed() {
    use smoltcp::{phy::Device, time::Instant};
    let (mut d, mut peer) = pair();
    peer.write_all(&[0, 0, 0, 14]).unwrap();
    peer.write_all(&[0x55; 14]).unwrap();
    peer.write_all(&[0, 0, 0, 14, 1]).unwrap();
    drop(peer);
    assert!(d.receive(Instant::from_millis(0)).is_none());
    assert!(d.error().is_some());
}
