#[macro_export]
macro_rules! poll_ready {
    ($expr:expr) => {
        match $expr {
            core::task::Poll::Ready(val) => val,
            core::task::Poll::Pending => {
                return core::task::Poll::Pending;
            }
        }
    };
}

#[macro_export]
macro_rules! poll_pin_ready {
    ($pin:expr, $context:expr) => {
        $crate::poll_ready!(::core::pin::pin!($pin).poll($context))
    };
}
