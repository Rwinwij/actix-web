use std::{
    fmt, io, net,
    pin::Pin,
    task::{Context, Poll},
};

use actix_codec::{AsyncRead, AsyncWrite, Framed, ReadBuf};
use actix_http::{
    body::Body,
    client::{Connect as ClientConnect, ConnectError, Connection, SendRequestError},
    h1::ClientCodec,
    RequestHead, RequestHeadType, ResponseHead,
};
use actix_service::Service;
//use futures_core::future::{BoxFuture, FutureExt};
use futures::future::{BoxFuture, FutureExt};

use crate::response::ClientResponse;

pub(crate) struct ConnectorWrapper<T>(pub T);

type TunnelResponse = (ResponseHead, Framed<BoxedSocket, ClientCodec>);

pub(crate) trait Connect {
    fn send_request(
        &self,
        head: RequestHeadType,
        body: Body,
        addr: Option<net::SocketAddr>,
    ) -> BoxFuture<'static, Result<ClientResponse, SendRequestError>>;

    /// Send request, returns Response and Framed
    fn open_tunnel(
        &self,
        head: RequestHead,
        addr: Option<net::SocketAddr>,
    ) -> BoxFuture<'static, Result<TunnelResponse, SendRequestError>>;
}

impl<T> Connect for ConnectorWrapper<T>
where
    T: Service<ClientConnect, Error = ConnectError> + Send,
    T::Response: Connection + Send,
    <T::Response as Connection>::Io: Send + 'static,
    <T::Response as Connection>::Future: Send + 'static,
    <T::Response as Connection>::TunnelFuture: Send + 'static,
    T::Future: 'static,
{
    fn send_request(
        &self,
        head: RequestHeadType,
        body: Body,
        addr: Option<net::SocketAddr>,
    ) -> BoxFuture<'static, Result<ClientResponse, SendRequestError>> {
        // connect to the host
        let fut = self.0.call(ClientConnect {
            uri: head.as_ref().uri.clone(),
            addr,
        });

        //Box::pin(async move {
        async move {
            let connection = fut.await?;

            // send request
            let (head, payload) = connection.send_request(head, body).await?;

            Ok(ClientResponse::new(head, payload))
        }
        .boxed()
        //})
    }

    fn open_tunnel(
        &self,
        head: RequestHead,
        addr: Option<net::SocketAddr>,
    ) -> BoxFuture<'static, Result<TunnelResponse, SendRequestError>> {
        // connect to the host
        let fut = self.0.call(ClientConnect {
            uri: head.uri.clone(),
            addr,
        });

        //Box::pin(async move {
        async move {
            let connection = fut.await?;

            // send request
            let (head, framed) =
                connection.open_tunnel(RequestHeadType::from(head)).await?;

            let framed = framed.into_map_io(|io| BoxedSocket(Box::new(Socket(io))));
            Ok((head, framed))
        }
        .boxed()
        //})
    }
}

trait AsyncSocket {
    fn as_read(&self) -> &(dyn AsyncRead + Unpin);
    fn as_read_mut(&mut self) -> &mut (dyn AsyncRead + Unpin);
    fn as_write(&mut self) -> &mut (dyn AsyncWrite + Unpin);
}

struct Socket<T: AsyncRead + AsyncWrite + Unpin>(T);

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncSocket for Socket<T> {
    fn as_read(&self) -> &(dyn AsyncRead + Unpin) {
        &self.0
    }
    fn as_read_mut(&mut self) -> &mut (dyn AsyncRead + Unpin) {
        &mut self.0
    }
    fn as_write(&mut self) -> &mut (dyn AsyncWrite + Unpin) {
        &mut self.0
    }
}

pub struct BoxedSocket(Box<dyn AsyncSocket>);

impl fmt::Debug for BoxedSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoxedSocket")
    }
}

impl AsyncRead for BoxedSocket {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().0.as_read_mut()).poll_read(cx, buf)
    }
}

impl AsyncWrite for BoxedSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.get_mut().0.as_write()).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().0.as_write()).poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().0.as_write()).poll_shutdown(cx)
    }
}
