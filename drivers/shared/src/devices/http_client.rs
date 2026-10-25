use super::{
    http_common::{create_tcp_connection, send_request, split_host_port},
    http_exchange::{ClientContext, Exchange, IO_TIMEOUT_SECONDS, read_response},
};
use alloc::{boxed::Box, vec::Vec};
use core::time::Duration;
use embassy_futures::select::{Either, select};
use file_system::{BaseOperations, CharacterDevice, Context, Error, MountOperations, Result, Size};
use shared::HttpRequestParser;
use synchronization::Arc;

async fn run_request(
    manager: &'static network::Manager,
    exchange: Arc<Exchange>,
    request: Vec<u8>,
    generation: u64,
) {
    let mut buffer = match exchange.take_buffer(generation).await {
        Ok(buffer) => buffer,
        Err(error) => {
            exchange.finish(generation, None, Err(error)).await;
            return;
        }
    };
    let operation = async {
        let parser = HttpRequestParser::from_buffer(&request);
        let (method, _) = parser.get_request().ok_or(Error::InvalidParameter)?;
        let head_request = method == "HEAD";
        let host = parser
            .get_headers()
            .find(|(n, _)| n.eq_ignore_ascii_case("Host"))
            .map(|(_, v)| v)
            .ok_or(Error::InvalidParameter)?;
        let (host, port) = split_host_port(host, 80);
        let mut socket = create_tcp_connection(manager, host, port).await?;
        match select(
            send_request(&mut socket, &request),
            task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
        )
        .await
        {
            Either::First(result) => result?,
            Either::Second(_) => return Err(Error::InputOutput),
        }
        drop(request);
        let result = read_response(
            &mut socket,
            &exchange,
            generation,
            &mut buffer,
            head_request,
        )
        .await;
        socket.socket.close_forced().await;
        result
    };
    let result = match select(operation, exchange.cancelled.wait()).await {
        Either::First(result) => result,
        Either::Second(_) => Err(Error::InvalidContext),
    };
    exchange.finish(generation, buffer, result).await;
}

pub struct HttpClientDevice {
    network_manager: &'static network::Manager,
    task_manager: &'static task::Manager,
}
impl HttpClientDevice {
    pub const fn new(
        network_manager: &'static network::Manager,
        task_manager: &'static task::Manager,
    ) -> Self {
        Self {
            network_manager,
            task_manager,
        }
    }
}
impl BaseOperations for HttpClientDevice {
    fn open(&self, context: &mut Context) -> Result<()> {
        context.set_private_data(Box::new(ClientContext::new()));
        Ok(())
    }
    fn close(&self, context: &mut Context) -> Result<()> {
        if let Some(mut client) = context.take_private_data_of_type::<ClientContext>() {
            client.cancel_read();
            task::block_on(client.exchange.close());
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
        context
            .get_private_data_mutable_of_type::<ClientContext>()
            .ok_or(Error::InvalidContext)?
            .poll_read(cx, buffer)
    }
    fn write(
        &self,
        context: &mut Context,
        _: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let client = context
                .get_private_data_mutable_of_type::<ClientContext>()
                .ok_or(Error::InvalidContext)?;
            let generation = client.exchange.begin()?;
            let request = buffer.to_vec();
            let exchange = client.exchange.clone();
            let manager = self.network_manager;

            if task::block_on(self.task_manager.spawn(
                task::Manager::ROOT_TASK_IDENTIFIER,
                "HTTP request worker",
                None,
                move |_| async move {
                    run_request(manager, exchange, request, generation).await;
                },
            ))
            .is_err()
            {
                task::block_on(
                    client
                        .exchange
                        .finish(generation, None, Err(Error::RessourceBusy)),
                );
                return Err(Error::RessourceBusy);
            }
            Ok(buffer.len())
        })
    }
    fn clone_context(&self, context: &Context) -> Result<Context> {
        let client = context
            .get_private_data_of_type::<ClientContext>()
            .ok_or(Error::InvalidContext)?;
        Ok(Context::new(Some(client.clone_context())))
    }
    fn cancel_operation(&self, context: &mut Context, operation: file_system::Operation) {
        if operation == file_system::Operation::Read
            && let Some(client) = context.get_private_data_mutable_of_type::<ClientContext>()
        {
            client.cancel_read();
        }
    }
}
impl MountOperations for HttpClientDevice {}
impl CharacterDevice for HttpClientDevice {}
