use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
use xila_http::{BodyDecoder, HttpRequestBuilder, HttpResponseBuilder, parse_response_head};
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
#[test]
fn parsing_decoding_and_writers_do_not_allocate() {
    let wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    COUNT.set(Some(0));
    for _ in 0..100 {
        let head = parse_response_head::<32>(wire, false).unwrap().unwrap();
        let mut decoder = BodyDecoder::new(head.body);
        let mut body = *b"5\r\nhello\r\n0\r\n\r\n";
        assert_eq!(decoder.decode(&mut body).unwrap(), 5);
        decoder.finish().unwrap();
        let mut buffer = [0; 128];
        let mut request = HttpRequestBuilder::from_buffer(&mut buffer);
        request
            .add_request("GET", "/", "HTTP/1.1")
            .unwrap()
            .add_header("Host", b"localhost")
            .unwrap()
            .add_body(b"")
            .unwrap();
        let mut response = HttpResponseBuilder::from_buffer(&mut buffer);
        response.add_status_code(200).unwrap();
        response.add_header("X", b"a").unwrap();
        response.add_chunk(b"hello").unwrap();
        response.finish_chunks().unwrap();
    }
    let count = COUNT.replace(None).unwrap();
    assert_eq!(count, 0);
}
