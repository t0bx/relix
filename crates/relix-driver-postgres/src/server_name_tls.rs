/*
 * @author t0bx
 * @since 23.10.2026
 */

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres_rustls::MakeRustlsConnect;

#[derive(Clone)]
pub(crate) struct ServerNameTls {
    inner: MakeRustlsConnect,
    server_name: Option<String>,
}

impl ServerNameTls {
    pub(crate) fn new(inner: MakeRustlsConnect, server_name: Option<String>) -> Self {
        Self { inner, server_name }
    }
}

impl<S> MakeTlsConnect<S> for ServerNameTls
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Stream = <MakeRustlsConnect as MakeTlsConnect<S>>::Stream;
    type TlsConnect = <MakeRustlsConnect as MakeTlsConnect<S>>::TlsConnect;
    type Error = <MakeRustlsConnect as MakeTlsConnect<S>>::Error;

    fn make_tls_connect(&mut self, domain: &str) -> Result<Self::TlsConnect, Self::Error> {
        let name = self.server_name.as_deref().unwrap_or(domain).to_owned();
        <MakeRustlsConnect as MakeTlsConnect<S>>::make_tls_connect(&mut self.inner, &name)
    }
}
