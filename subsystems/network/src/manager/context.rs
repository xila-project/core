use core::task::{Context, Poll};

use crate::manager::stack::Stack;
use smoltcp::iface::SocketHandle;

pub struct SocketContext {
    pub handle: SocketHandle,
    pub stack: Stack,
    pub closed: bool,
}

impl SocketContext {
    pub async fn with<F, S, R>(&self, f: F) -> R
    where
        F: FnOnce(&S) -> R,
        S: smoltcp::socket::AnySocket<'static>,
    {
        self.stack
            .with(|stack_inner| {
                let socket = stack_inner.sockets.get::<S>(self.handle);
                f(socket)
            })
            .await
    }

    pub async fn with_mutable<F, S, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut S) -> R,
        S: smoltcp::socket::AnySocket<'static>,
    {
        self.stack
            .with_mutable(|stack_inner| {
                let socket = stack_inner.sockets.get_mut::<S>(self.handle);
                f(socket)
            })
            .await
    }

    pub fn poll_with<F, S, R>(&self, context: &mut Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&S, &mut Context<'_>) -> Poll<R>,
        S: smoltcp::socket::AnySocket<'static>,
    {
        self.stack.poll_with(context, |stack_inner, context| {
            let socket = stack_inner.sockets.get::<S>(self.handle);

            f(socket, context)
        })
    }

    pub fn poll_with_mutable<F, S, R>(&self, context: &mut core::task::Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&mut S, &mut Context<'_>) -> Poll<R>,
        S: smoltcp::socket::AnySocket<'static>,
    {
        self.stack
            .poll_with_mutable(context, |stack_inner, context| {
                let socket = stack_inner.sockets.get_mut::<S>(self.handle);

                f(socket, context)
            })
    }
}
