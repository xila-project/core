use super::*;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Wake,
};

#[derive(Default)]
struct Counter(AtomicUsize);
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn counter() -> (Arc<Counter>, Waker) {
    let count = Arc::new(Counter::default());
    let waker = Waker::from(count.clone());
    (count, waker)
}

#[task::test]
async fn vfs_pending_read_is_woken_once_and_cancel_unregisters() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::empty())
        .await
        .unwrap();
    let (count, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut byte = [0];
    {
        let mut read = pin!(reader.read(&mut byte));
        for _ in 0..3 {
            assert!(read.as_mut().poll(&mut cx).is_pending());
        }
        assert_eq!(count.0.load(Ordering::SeqCst), 0);
        writer.write(b"x").await.unwrap();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(read.as_mut().poll(&mut cx), Poll::Ready(Ok(1)));
    }
    assert_eq!(&byte, b"x");
    {
        let mut cancelled = pin!(reader.read(&mut byte));
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
    }
    writer.write(b"y").await.unwrap();
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert_eq!(reader.read(&mut byte).await.unwrap(), 1);
    assert_eq!(&byte, b"y");
}

#[task::test]
async fn vfs_full_write_waits_then_reports_last_reader_close() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::empty())
        .await
        .unwrap();
    writer.write(b"x").await.unwrap();
    let (count, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    {
        let mut write = pin!(writer.write(b"y"));
        assert!(write.as_mut().poll(&mut cx).is_pending());
        let mut byte = [0];
        reader.read(&mut byte).await.unwrap();
        assert_eq!(&byte, b"x");
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(write.as_mut().poll(&mut cx), Poll::Ready(Ok(1)));
    }
    let mut write = pin!(writer.write(b"z"));
    assert!(write.as_mut().poll(&mut cx).is_pending());
    reader.close(vfs).await.unwrap();
    assert_eq!(count.0.load(Ordering::SeqCst), 2);
    assert_eq!(
        write.as_mut().poll(&mut cx),
        Poll::Ready(Err(virtual_file_system::Error::FileSystem(
            file_system::Error::InputOutput
        )))
    );
}

#[task::test]
async fn vfs_nonblocking_read_and_write_return_busy_without_waking() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::NonBlocking)
        .await
        .unwrap();
    let (count, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut byte = [0];
    let busy = Poll::Ready(Err(virtual_file_system::Error::FileSystem(
        file_system::Error::RessourceBusy,
    )));
    {
        let mut read = pin!(reader.read(&mut byte));
        assert_eq!(read.as_mut().poll(&mut cx), busy);
    }
    writer.write(b"a").await.unwrap();
    {
        let mut write = pin!(writer.write(b"b"));
        assert_eq!(write.as_mut().poll(&mut cx), busy);
    }
    reader.read(&mut byte).await.unwrap();
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
}

#[task::test]
async fn vfs_duplicate_readers_wake_independently_and_last_writer_produces_eof() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 2, StateFlags::empty())
        .await
        .unwrap();
    let mut second = reader.duplicate().await.unwrap();
    let mut last_writer = writer.duplicate().await.unwrap();
    let (first_count, first_waker) = counter();
    let (second_count, second_waker) = counter();
    let mut first_cx = Context::from_waker(&first_waker);
    let mut second_cx = Context::from_waker(&second_waker);
    let mut first_byte = [0];
    let mut second_byte = [0];
    {
        let mut first_read = pin!(reader.read(&mut first_byte));
        let mut second_read = pin!(second.read(&mut second_byte));
        assert!(first_read.as_mut().poll(&mut first_cx).is_pending());
        assert!(second_read.as_mut().poll(&mut second_cx).is_pending());
        writer.write(b"ab").await.unwrap();
        assert_eq!(first_count.0.load(Ordering::SeqCst), 1);
        assert_eq!(second_count.0.load(Ordering::SeqCst), 1);
        assert_eq!(first_read.as_mut().poll(&mut first_cx), Poll::Ready(Ok(1)));
        assert_eq!(
            second_read.as_mut().poll(&mut second_cx),
            Poll::Ready(Ok(1))
        );
    }
    assert_eq!(&first_byte, b"a");
    assert_eq!(&second_byte, b"b");
    writer.close(vfs).await.unwrap();
    {
        let mut read = pin!(reader.read(&mut first_byte));
        assert!(read.as_mut().poll(&mut first_cx).is_pending());
        last_writer.write(b"c").await.unwrap();
        last_writer.close(vfs).await.unwrap();
        assert_eq!(read.as_mut().poll(&mut first_cx), Poll::Ready(Ok(1)));
    }
    assert_eq!(reader.read(&mut first_byte).await.unwrap(), 0);
}

#[task::test]
async fn vfs_write_line_keeps_partial_progress_across_waits() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::empty())
        .await
        .unwrap();
    let (_, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut byte = [0];
    let mut output = std::vec::Vec::new();
    let mut line = pin!(writer.write_line(b"ab"));
    for expected in b"ab" {
        assert!(line.as_mut().poll(&mut cx).is_pending());
        assert_eq!(reader.read(&mut byte).await.unwrap(), 1);
        assert_eq!(byte[0], *expected);
        output.push(byte[0]);
    }
    assert_eq!(line.as_mut().poll(&mut cx), Poll::Ready(Ok(3)));
    assert_eq!(reader.read(&mut byte).await.unwrap(), 1);
    output.push(byte[0]);
    assert_eq!(&output, b"ab\n");
}

#[task::test]
async fn vfs_read_to_end_keeps_bytes_and_cursor_across_waits() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 2, StateFlags::empty())
        .await
        .unwrap();
    writer.write(b"ab").await.unwrap();
    let (count, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut output = std::vec::Vec::new();
    {
        let mut read = pin!(reader.read_to_end(&mut output, 2));
        assert!(read.as_mut().poll(&mut cx).is_pending());
        writer.write(b"cd").await.unwrap();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert!(read.as_mut().poll(&mut cx).is_pending());
        writer.close(vfs).await.unwrap();
        assert_eq!(read.as_mut().poll(&mut cx), Poll::Ready(Ok(4)));
    }
    assert_eq!(&output, b"abcd");
    assert_eq!(reader.set_position(&Position::Current(0)).await.unwrap(), 4);
}

#[task::test]
async fn vfs_read_until_waits_for_first_byte_and_returns_available_progress() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 8, StateFlags::empty())
        .await
        .unwrap();
    let (count, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut output = [0; 8];
    {
        let mut read = pin!(reader.read_until(&mut output, b"\r\n"));
        assert!(read.as_mut().poll(&mut cx).is_pending());
        assert_eq!(count.0.load(Ordering::SeqCst), 0);
        writer.write(b"ab\r\nx").await.unwrap();
        assert_eq!(read.as_mut().poll(&mut cx), Poll::Ready(Ok(4)));
    }
    assert_eq!(&output[..4], b"ab\r\n");
    assert_eq!(reader.read_until(&mut output, b"\r\n").await.unwrap(), 1);
    assert_eq!(output[0], b'x');
    assert_eq!(reader.set_position(&Position::Current(0)).await.unwrap(), 5);
    assert!(reader.read_until(&mut output, b"").await.is_err());
}

#[task::test]
async fn vfs_steady_state_poll_wake_and_cancel_allocate_nothing() {
    let (_, vfs) = initialize().await;
    let (reader, mut writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::empty())
        .await
        .unwrap();
    let mut reader = reader.duplicate().await.unwrap(); // Exercise shared cursor storage.
    let (_, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut byte = [0];
    let requests = super::allocation::measure(|| {
        for _ in 0..128 {
            {
                let mut cancelled = pin!(reader.read(&mut byte));
                assert!(cancelled.as_mut().poll(&mut cx).is_pending());
                assert!(cancelled.as_mut().poll(&mut cx).is_pending());
            }
            {
                let mut read = pin!(reader.read(&mut byte));
                assert!(read.as_mut().poll(&mut cx).is_pending());
                assert_eq!(
                    pin!(writer.write(b"x")).as_mut().poll(&mut cx),
                    Poll::Ready(Ok(1))
                );
                assert_eq!(read.as_mut().poll(&mut cx), Poll::Ready(Ok(1)));
            }
            assert_eq!(
                pin!(writer.write(b"y")).as_mut().poll(&mut cx),
                Poll::Ready(Ok(1))
            );
            {
                let mut write = pin!(writer.write(b"z"));
                assert!(write.as_mut().poll(&mut cx).is_pending());
                assert_eq!(
                    pin!(reader.read(&mut byte)).as_mut().poll(&mut cx),
                    Poll::Ready(Ok(1))
                );
                assert_eq!(write.as_mut().poll(&mut cx), Poll::Ready(Ok(1)));
            }
            assert_eq!(
                pin!(reader.read(&mut byte)).as_mut().poll(&mut cx),
                Poll::Ready(Ok(1))
            );
        }
    });
    assert_eq!(requests, 0);
}

#[task::test]
async fn external_deadline_cancels_pending_registration() {
    use core::sync::atomic::AtomicBool;
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::empty())
        .await
        .unwrap();
    let expired = AtomicBool::new(false);
    let (count, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut byte = [0];
    {
        let mut deadline = pin!(async {
            let mut read = pin!(reader.read(&mut byte));
            core::future::poll_fn(|cx| {
                if expired.load(Ordering::SeqCst) {
                    Poll::Ready(None)
                } else {
                    read.as_mut().poll(cx).map(Some)
                }
            })
            .await
        });
        assert!(deadline.as_mut().poll(&mut cx).is_pending());
        expired.store(true, Ordering::SeqCst); // Deterministic deadline event.
        assert_eq!(deadline.as_mut().poll(&mut cx), Poll::Ready(None));
    }
    writer.write(b"x").await.unwrap();
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    assert_eq!(reader.read(&mut byte).await.unwrap(), 1);
}

#[task::test]
async fn cancelling_compound_read_preserves_consumed_bytes_and_cursor() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 2, StateFlags::empty())
        .await
        .unwrap();
    writer.write(b"ab").await.unwrap();
    let (count, waker) = counter();
    let mut cx = Context::from_waker(&waker);
    let mut output = std::vec::Vec::new();
    {
        let mut read = pin!(reader.read_to_end(&mut output, 2));
        assert!(read.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(&output, b"ab");
    assert_eq!(reader.set_position(&Position::Current(0)).await.unwrap(), 2);
    writer.write(b"cd").await.unwrap();
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    writer.close(vfs).await.unwrap();
    assert_eq!(reader.read_to_end(&mut output, 2).await.unwrap(), 2);
    assert_eq!(&output, b"abcd");
    assert!(reader.read_to_end(&mut output, 0).await.is_err());
}

#[task::test]
async fn nonblocking_compound_helpers_return_partial_progress() {
    let (_, vfs) = initialize().await;
    let (mut reader, mut writer) = File::create_unnamed_pipe(vfs, 2, StateFlags::NonBlocking)
        .await
        .unwrap();
    assert_eq!(writer.write_line(b"abcd").await.unwrap(), 2);
    let mut bytes = std::vec::Vec::new();
    assert_eq!(reader.read_to_end(&mut bytes, 1).await.unwrap(), 2);
    assert_eq!(&bytes, b"ab");
    assert!(reader.read_to_end(&mut bytes, 1).await.is_err());
}

#[task::test]
async fn duplicate_capacity_failure_leaves_existing_descriptors_usable() {
    let (_, vfs) = initialize().await;
    let (reader, mut writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::empty())
        .await
        .unwrap();
    let mut readers = std::vec::Vec::from([reader]);
    for _ in 0..14 {
        readers.push(readers[0].duplicate().await.unwrap());
    }
    assert!(matches!(
        readers[0].duplicate().await,
        Err(virtual_file_system::Error::FileSystem(
            file_system::Error::TooManyOpenFiles
        ))
    ));
    readers.pop().unwrap().close(vfs).await.unwrap();
    let mut replacement = readers[0].duplicate().await.unwrap();
    writer.write(b"x").await.unwrap();
    let mut byte = [0];
    assert_eq!(replacement.read(&mut byte).await.unwrap(), 1);
    assert_eq!(&byte, b"x");
}
