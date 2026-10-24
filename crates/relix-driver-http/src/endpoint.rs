/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{map_status_error, map_transport_error};
use relix_core::{ConnectionConfig, Secret, TlsMode};
use relix_driver_api::{ConnectionProperty, DriverError, ErrorKind};
use reqwest::{Client, ClientBuilder, Method, RequestBuilder, Response, Url};
use serde::de::DeserializeOwned;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

pub const PATH_PREFIX: ConnectionProperty = ConnectionProperty::optional(
    "path_prefix",
    "URL path prepended to every request, for servers behind a reverse proxy",
);

#[derive(Clone, Default)]
pub enum HttpAuth {
    #[default]
    None,
    Basic {
        user: String,
        password: Option<Secret>,
    },
    Bearer(Secret),
    Header {
        name: String,
        value: Secret,
    },
}

impl HttpAuth {
    pub fn basic_from(config: &ConnectionConfig) -> Self {
        match config.settings.username.as_deref().map(str::trim) {
            Some(user) if !user.is_empty() => Self::Basic {
                user: user.to_owned(),
                password: config.password.clone(),
            },
            _ => Self::None,
        }
    }

    fn apply(&self, request: RequestBuilder) -> RequestBuilder {
        match self {
            Self::None => request,
            Self::Basic { user, password } => {
                request.basic_auth(user, password.as_ref().map(Secret::expose))
            }
            Self::Bearer(token) => request.bearer_auth(token.expose()),
            Self::Header { name, value } => request.header(name.as_str(), value.expose()),
        }
    }
}

#[derive(Clone)]
pub struct EndpointOptions {
    pub default_port: u16,
    pub probe_path: String,
    pub auth: HttpAuth,
    pub request_timeout: Option<Duration>,
}

#[derive(Clone)]
pub struct HttpEndpoint {
    client: Client,
    base: Url,
    auth: HttpAuth,
}

impl HttpEndpoint {
    /// Finds a working scheme for the TLS mode (`prefer` tries HTTPS, then HTTP) by
    /// requesting the engine's probe path, and returns an endpoint bound to it. Behind
    /// an SSH tunnel the URL keeps the original host name and DNS for it is pinned to the
    /// forwarded address, so certificates are verified against the real server name.
    pub async fn connect(
        config: &ConnectionConfig,
        options: EndpointOptions,
    ) -> Result<Self, DriverError> {
        let schemes: &[&str] = match config.settings.tls.mode {
            TlsMode::Disable => &["http"],
            TlsMode::Prefer => &["https", "http"],
            TlsMode::Require | TlsMode::VerifyFull => &["https"],
        };
        let path_prefix = config
            .settings
            .property(PATH_PREFIX.name)
            .map(str::to_owned);
        let mut last_error = None;
        for scheme in schemes {
            let endpoint = Self::build(config, &options, scheme, path_prefix.as_deref()).await?;
            let probe = endpoint.request(Method::GET, &options.probe_path);
            match probe.send().await {
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    let status = response.status().as_u16();
                    let body = response.text().await.unwrap_or_default();
                    return Err(map_status_error(status, &body));
                }
                Ok(_) => return Ok(endpoint),
                Err(error) => {
                    let mapped = map_transport_error(&error);
                    let retry = matches!(
                        mapped.kind,
                        ErrorKind::Tls
                            | ErrorKind::Connection
                            | ErrorKind::Io
                            | ErrorKind::Protocol
                    );
                    last_error = Some(mapped);
                    if !retry {
                        break;
                    }
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| DriverError::internal("no HTTP connection attempt was made")))
    }

    async fn build(
        config: &ConnectionConfig,
        options: &EndpointOptions,
        scheme: &str,
        path_prefix: Option<&str>,
    ) -> Result<Self, DriverError> {
        relix_tls::install_default_provider();
        let host = config.tls_host().to_owned();
        let port = config.settings.port.unwrap_or(options.default_port);
        let mut builder = client_builder(config, options.request_timeout);
        if scheme == "https" {
            let tls = relix_tls::client_config(&config.settings.tls)
                .await?
                .ok_or_else(|| DriverError::internal("TLS is disabled for this data source"))?;
            builder = builder.tls_backend_preconfigured(tls);
        }
        let client = pin_tunneled_host(builder, config, &host)
            .build()
            .map_err(|error| DriverError::configuration(error.to_string()))?;
        let host_part = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let prefix = path_prefix
            .map(|prefix| prefix.trim_matches('/'))
            .filter(|prefix| !prefix.is_empty())
            .map(|prefix| format!("{prefix}/"))
            .unwrap_or_default();
        let base =
            Url::parse(&format!("{scheme}://{host_part}:{port}/{prefix}")).map_err(|error| {
                DriverError::configuration(format!("invalid server address: {error}"))
            })?;
        Ok(Self {
            client,
            base,
            auth: options.auth.clone(),
        })
    }

    pub(crate) fn from_parts(client: Client, base: Url, auth: HttpAuth) -> Self {
        Self { client, base, auth }
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }

    pub fn is_https(&self) -> bool {
        self.base.scheme() == "https"
    }

    pub fn url(&self, path: &str) -> Url {
        self.base
            .join(path.trim_start_matches('/'))
            .unwrap_or_else(|_| self.base.clone())
    }

    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.request_url(method, self.url(path))
    }

    pub fn request_url(&self, method: Method, url: Url) -> RequestBuilder {
        self.auth.apply(self.client.request(method, url))
    }

    pub fn with_auth(&self, auth: HttpAuth) -> Self {
        Self {
            auth,
            ..self.clone()
        }
    }

    pub async fn send(&self, request: RequestBuilder) -> Result<Response, DriverError> {
        let response = request
            .send()
            .await
            .map_err(|error| map_transport_error(&error))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().await.unwrap_or_default();
        Err(map_status_error(status.as_u16(), &body))
    }

    pub async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T, DriverError> {
        let response = self.send(request).await?;
        response
            .json::<T>()
            .await
            .map_err(|error| map_transport_error(&error))
    }

    pub async fn text(&self, request: RequestBuilder) -> Result<String, DriverError> {
        let response = self.send(request).await?;
        response
            .text()
            .await
            .map_err(|error| map_transport_error(&error))
    }
}

pub(crate) fn client_builder(
    config: &ConnectionConfig,
    request_timeout: Option<Duration>,
) -> ClientBuilder {
    let builder = Client::builder()
        .connect_timeout(config.settings.connect_timeout())
        .user_agent(concat!("relix/", env!("CARGO_PKG_VERSION")));
    match request_timeout {
        Some(timeout) => builder.timeout(timeout),
        None => builder,
    }
}

/// Behind an SSH tunnel the configured host is the forwarded loopback address while URLs
/// keep the original host name, so DNS for that name is pinned to the forwarded address.
pub(crate) fn pin_tunneled_host(
    builder: ClientBuilder,
    config: &ConnectionConfig,
    host: &str,
) -> ClientBuilder {
    if config.tls_server_name.is_some()
        && let Ok(ip) = config.host_or_default().parse::<IpAddr>()
    {
        return builder.resolve(host, SocketAddr::new(ip, 0));
    }
    builder
}
