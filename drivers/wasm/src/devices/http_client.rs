use ::alloc::boxed::Box;
use alloc::{format, rc::Rc};
use core::cell::RefCell;
use file_system::{BaseOperations, CharacterDevice, Context, Error, MountOperations, Result, Size};
use shared::{HttpRequestParser, HttpResponseBuilder};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Headers, Request, RequestInit};

enum State {
    Idle,
    Failed(Error),
    RequestPending,
    RequestReady(web_sys::Response),
    BodyPending,
    BodyReady(js_sys::Uint8Array),
}

struct HttpClientContext {
    state: Rc<RefCell<State>>,
    waker: Rc<RefCell<Option<core::task::Waker>>>,
}

impl Clone for HttpClientContext {
    fn clone(&self) -> Self {
        HttpClientContext {
            state: Rc::new(RefCell::new(State::Idle)),
            waker: Rc::new(RefCell::new(None)),
        }
    }
}

unsafe impl Send for HttpClientContext {}
unsafe impl Sync for HttpClientContext {}

fn complete(state: &RefCell<State>, waker: &RefCell<Option<core::task::Waker>>, next: State) {
    *state.borrow_mut() = next;
    let pending = waker.borrow_mut().take();
    if let Some(waker) = pending {
        waker.wake();
    }
}

fn build_request<'a>(scheme: &str, request: HttpRequestParser<'a>) -> Option<Request> {
    let options = RequestInit::new();
    let options_headers = Headers::new().ok()?;

    let mut host: Option<&str> = None;
    let mut path: Option<&str> = None;

    if let Some((m, p)) = request.get_request() {
        options.set_method(m);
        path = Some(p);
    }

    for item in request.get_headers() {
        let (name, value) = item;

        match name {
            HttpRequestParser::HOST_HEADER => {
                host = Some(value);
            }
            HttpRequestParser::CONNECTION_HEADER => {
                // Skip connection header
            }
            _ => {
                if let Err(e) = options_headers.append(name, value) {
                    log::warning!("Failed to append header {}: {:?}", name, e);
                }
            }
        }
    }

    if let Some(body) = request.get_body() {
        let concat_body = body.to_vec();
        options.set_body(&concat_body.into());
    }

    options.set_headers(&options_headers);

    let url = format!("{}://{}{}", scheme, host?, path.unwrap_or("/"));

    Request::new_with_str_and_init(&url, &options).ok()
}

fn build_headers_response(response: &web_sys::Response, buffer: &mut [u8]) -> Result<usize> {
    let mut builder = HttpResponseBuilder::from_buffer(buffer);

    // Status code
    let status = response.status();
    builder
        .add_status_code(status)
        .ok_or(Error::InternalError)?;

    // Headers
    for header in response.headers().entries() {
        let header = match header {
            Ok(h) => h,
            Err(_) => continue,
        };

        let header_array: js_sys::Array = header.dyn_into().map_err(|_| Error::InternalError)?;
        if header_array.length() != 2 {
            continue;
        }

        let name = header_array
            .get(0)
            .as_string()
            .ok_or(Error::InternalError)?;
        let value = header_array
            .get(1)
            .as_string()
            .ok_or(Error::InternalError)?;

        builder
            .add_header(&name, value.as_bytes())
            .ok_or(Error::InternalError)?;
    }

    // End of headers
    builder.add_line(b"").ok_or(Error::InternalError)?;

    Ok(builder.get_position())
}

pub struct HttpClientDevice;

unsafe impl Send for HttpClientDevice {}
unsafe impl Sync for HttpClientDevice {}

impl BaseOperations for HttpClientDevice {
    fn open(&self, context: &mut Context) -> Result<()> {
        context.set_private_data(Box::new(HttpClientContext {
            state: Rc::new(RefCell::new(State::Idle)),
            waker: Rc::new(RefCell::new(None)),
        }));

        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        if let Some(context) = context.take_private_data_of_type::<HttpClientContext>() {
            let retired = context.waker.replace(None);
            drop(retired);
        }

        Ok(())
    }

    fn read(
        &self,
        context: &mut Context,
        cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        use core::task::Poll;
        let context = context
            .get_private_data_mutable_of_type::<HttpClientContext>()
            .ok_or(Error::InvalidContext)?;
        if buffer.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let replacement = cx.waker().clone();
        let retired = context.waker.replace(Some(replacement));
        drop(retired);
        let mut state = context.state.borrow_mut();
        match core::mem::replace(&mut *state, State::Idle) {
            State::RequestPending => {
                *state = State::RequestPending;
                Poll::Pending
            }
            State::BodyPending => {
                *state = State::BodyPending;
                Poll::Pending
            }
            State::Failed(error) => Poll::Ready(Err(error)),
            State::RequestReady(response) => {
                let count = build_headers_response(&response, buffer)?;
                *state = State::BodyPending;
                let state = context.state.clone();
                let waker = context.waker.clone();
                spawn_local(async move {
                    let result = async {
                        let promise = response.array_buffer().map_err(|_| Error::InputOutput)?;
                        let body = JsFuture::from(promise)
                            .await
                            .map_err(|_| Error::InputOutput)?;
                        let body = body
                            .dyn_into::<js_sys::ArrayBuffer>()
                            .map_err(|_| Error::InputOutput)?;
                        Ok::<_, Error>(js_sys::Uint8Array::new(&body))
                    }
                    .await;
                    complete(
                        &state,
                        &waker,
                        match result {
                            Ok(body) => State::BodyReady(body),
                            Err(error) => State::Failed(error),
                        },
                    );
                });
                Poll::Ready(Ok(count))
            }
            State::BodyReady(body) => {
                let length = (body.length() as usize).min(buffer.len());
                body.subarray(0, length as u32)
                    .copy_to(&mut buffer[..length]);
                if length < body.length() as usize {
                    *state = State::BodyReady(body.subarray(length as u32, body.length()));
                }
                Poll::Ready(Ok(length))
            }
            State::Idle => Poll::Ready(Ok(0)),
        }
    }

    fn write(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            let context = context
                .get_private_data_mutable_of_type::<HttpClientContext>()
                .ok_or(Error::InvalidContext)?;
            if !matches!(*context.state.borrow(), State::Idle | State::Failed(_)) {
                return Err(Error::RessourceBusy);
            }
            let request = build_request("https", HttpRequestParser::from_buffer(buffer))
                .ok_or(Error::InvalidParameter)?;
            *context.state.borrow_mut() = State::RequestPending;
            let state = context.state.clone();
            let waker = context.waker.clone();
            spawn_local(async move {
                let result = async {
                    let window = web_sys::window().ok_or(Error::InputOutput)?;
                    JsFuture::from(window.fetch_with_request(&request))
                        .await
                        .map_err(|_| Error::InputOutput)?
                        .dyn_into::<web_sys::Response>()
                        .map_err(|_| Error::InputOutput)
                }
                .await;
                complete(
                    &state,
                    &waker,
                    match result {
                        Ok(response) => State::RequestReady(response),
                        Err(error) => State::Failed(error),
                    },
                );
            });
            Ok(buffer.len())
        })
    }

    fn clone_context(&self, context: &Context) -> file_system::Result<Context> {
        let context = context
            .get_private_data_of_type::<HttpClientContext>()
            .ok_or(file_system::Error::InvalidParameter)?;

        Ok(Context::new(Some(context.clone())))
    }
    fn cancel_operation(&self, context: &mut Context, operation: file_system::Operation) {
        if operation == file_system::Operation::Read
            && let Some(context) = context.get_private_data_mutable_of_type::<HttpClientContext>()
        {
            let retired = context.waker.replace(None);
            drop(retired);
        }
    }
}

impl MountOperations for HttpClientDevice {}

impl CharacterDevice for HttpClientDevice {}

#[cfg(test)]
mod polling_tests {
    use super::*;
    use core::task::{Context as TaskContext, Poll, Waker};
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn a_pending_browser_request_uses_the_device_polling_contract() {
        let device = HttpClientDevice;
        let mut context = Context::new_empty();
        device.open(&mut context).unwrap();
        *context
            .get_private_data_mutable_of_type::<HttpClientContext>()
            .unwrap()
            .state
            .borrow_mut() = State::RequestPending;
        let mut cx = TaskContext::from_waker(Waker::noop());
        assert_eq!(
            BaseOperations::read(&device, &mut context, &mut cx, &mut [0; 8], 0),
            Poll::Pending
        );
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn completion_wakes_the_reader_and_cancellation_removes_registration() {
        use alloc::sync::Arc;
        use core::sync::atomic::{AtomicUsize, Ordering};
        struct Counter(AtomicUsize);
        impl alloc::task::Wake for Counter {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut cx = TaskContext::from_waker(&waker);
        let device = HttpClientDevice;
        let mut context = Context::new_empty();
        device.open(&mut context).unwrap();
        let inner = context
            .get_private_data_of_type::<HttpClientContext>()
            .unwrap();
        let state = inner.state.clone();
        let registration = inner.waker.clone();
        *state.borrow_mut() = State::RequestPending;
        assert_eq!(
            BaseOperations::read(&device, &mut context, &mut cx, &mut [0; 8], 0),
            Poll::Pending
        );
        complete(&state, &registration, State::Failed(Error::InputOutput));
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            BaseOperations::read(&device, &mut context, &mut cx, &mut [0; 8], 0),
            Poll::Ready(Err(Error::InputOutput))
        );
        *state.borrow_mut() = State::RequestPending;
        assert_eq!(
            BaseOperations::read(&device, &mut context, &mut cx, &mut [0; 8], 0),
            Poll::Pending
        );
        device.cancel_operation(&mut context, file_system::Operation::Read);
        complete(&state, &registration, State::Idle);
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    }
}
