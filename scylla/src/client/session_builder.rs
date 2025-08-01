//! SessionBuilder provides an easy way to create new Sessions

#[cfg(feature = "unstable-cloud")]
use super::execution_profile::ExecutionProfile;
use super::execution_profile::ExecutionProfileHandle;
use super::session::{Session, SessionConfig};
use super::{Compression, PoolSize, SelfIdentity, WriteCoalescingDelay};
use crate::authentication::{AuthenticatorProvider, PlainTextAuthenticator};
use crate::client::session::TlsContext;
#[cfg(feature = "unstable-cloud")]
use crate::cloud::{CloudConfig, CloudConfigError, CloudTlsProvider};
use crate::errors::NewSessionError;
use crate::policies::address_translator::AddressTranslator;
use crate::policies::host_filter::HostFilter;
use crate::policies::timestamp_generator::TimestampGenerator;
use crate::routing::ShardAwarePortRange;
use crate::statement::Consistency;
use std::borrow::Borrow;
use std::marker::PhantomData;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
#[cfg(feature = "unstable-cloud")]
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

mod sealed {
    // This is a sealed trait - its whole purpose is to be unnameable.
    // This means we need to disable the check.
    #[expect(unnameable_types)]
    pub trait Sealed {}
}
/// Kind of the session builder, used to distinguish between
/// builders that create regular sessions and those that create custom
/// sessions, such as cloud sessions.
/// This is used to conditionally enable different sets of methods
/// on the session builder based on its kind.
pub trait SessionBuilderKind: sealed::Sealed + Clone {}

/// Default session builder kind, used to create regular sessions.
#[derive(Clone)]
pub enum DefaultMode {}
impl sealed::Sealed for DefaultMode {}
impl SessionBuilderKind for DefaultMode {}

/// Builder for regular sessions.
pub type SessionBuilder = GenericSessionBuilder<DefaultMode>;

/// Session builder kind for creating cloud sessions.
#[cfg(feature = "unstable-cloud")]
#[derive(Clone)]
pub enum CloudMode {}
#[cfg(feature = "unstable-cloud")]
impl sealed::Sealed for CloudMode {}
#[cfg(feature = "unstable-cloud")]
impl SessionBuilderKind for CloudMode {}

/// Builder for sessions that connect to Scylla Cloud.
#[cfg(feature = "unstable-cloud")]
pub type CloudSessionBuilder = GenericSessionBuilder<CloudMode>;

/// Used to conveniently configure new Session instances.
///
/// Most likely you will want to use [`SessionBuilder`]
/// (for building regular session). If you want to build a session
/// that connects to Scylla Cloud, you will want to use
/// `CloudSessionBuilder`.
///
/// # Example
///
/// ```
/// # use scylla::client::session::Session;
/// # use scylla::client::session_builder::SessionBuilder;
/// # use scylla::client::Compression;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let session: Session = SessionBuilder::new()
///     .known_node("127.0.0.1:9042")
///     .compression(Some(Compression::Snappy))
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct GenericSessionBuilder<Kind: SessionBuilderKind> {
    /// Configuration for the session being built.
    pub config: SessionConfig,
    kind: PhantomData<Kind>,
}

// NOTE: this `impl` block contains configuration options specific for **non-Cloud** [`Session`].
// This means that if an option fits both non-Cloud and Cloud `Session`s, it should NOT be put
// here, but rather in `impl<K> GenericSessionBuilder<K>` block.
impl GenericSessionBuilder<DefaultMode> {
    /// Creates new SessionBuilder with default configuration
    /// # Default configuration
    /// * Compression: None
    ///
    pub fn new() -> Self {
        SessionBuilder {
            config: SessionConfig::new(),
            kind: PhantomData,
        }
    }

    /// Add a known node with a hostname
    /// # Examples
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("db1.example.com")
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn known_node(mut self, hostname: impl AsRef<str>) -> Self {
        self.config.add_known_node(hostname);
        self
    }

    /// Add a known node with an IP address
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node_addr(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9042))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn known_node_addr(mut self, node_addr: SocketAddr) -> Self {
        self.config.add_known_node_addr(node_addr);
        self
    }

    /// Add a list of known nodes with hostnames
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_nodes(["127.0.0.1:9042", "db1.example.com"])
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn known_nodes(mut self, hostnames: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.config.add_known_nodes(hostnames);
        self
    }

    /// Add a list of known nodes with IP addresses
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 3)), 9042);
    /// let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 4)), 9042);
    ///
    /// let session: Session = SessionBuilder::new()
    ///     .known_nodes_addr([addr1, addr2])
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn known_nodes_addr(
        mut self,
        node_addrs: impl IntoIterator<Item = impl Borrow<SocketAddr>>,
    ) -> Self {
        self.config.add_known_nodes_addr(node_addrs);
        self
    }

    /// Set username and password for plain text authentication.\
    /// If the database server will require authentication\
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::client::Compression;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .use_keyspace("my_keyspace_name", false)
    ///     .user("cassandra", "cassandra")
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn user(mut self, username: impl Into<String>, passwd: impl Into<String>) -> Self {
        self.config.authenticator = Some(Arc::new(PlainTextAuthenticator::new(
            username.into(),
            passwd.into(),
        )));
        self
    }

    /// Set custom authenticator provider to create an authenticator instance during a session creation.
    ///
    /// # Example
    /// ```
    /// # use std::sync::Arc;
    /// use bytes::Bytes;
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// use async_trait::async_trait;
    /// use scylla::authentication::{AuthenticatorProvider, AuthenticatorSession, AuthError};
    /// # use scylla::client::Compression;
    ///
    /// struct CustomAuthenticator;
    ///
    /// #[async_trait]
    /// impl AuthenticatorSession for CustomAuthenticator {
    ///     async fn evaluate_challenge(&mut self, token: Option<&[u8]>) -> Result<Option<Vec<u8>>, AuthError> {
    ///         Ok(None)
    ///     }
    ///
    ///     async fn success(&mut self, token: Option<&[u8]>) -> Result<(), AuthError> {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// struct CustomAuthenticatorProvider;
    ///
    /// #[async_trait]
    /// impl AuthenticatorProvider for CustomAuthenticatorProvider {
    ///     async fn start_authentication_session(&self, _authenticator_name: &str) -> Result<(Option<Vec<u8>>, Box<dyn AuthenticatorSession>), AuthError> {
    ///         Ok((None, Box::new(CustomAuthenticator)))
    ///     }
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .use_keyspace("my_keyspace_name", false)
    ///     .user("cassandra", "cassandra")
    ///     .authenticator_provider(Arc::new(CustomAuthenticatorProvider))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn authenticator_provider(
        mut self,
        authenticator_provider: Arc<dyn AuthenticatorProvider>,
    ) -> Self {
        self.config.authenticator = Some(authenticator_provider);
        self
    }

    /// Uses a custom address translator for peer addresses retrieved from the cluster.
    /// By default, no translation is performed.
    ///
    /// # Example
    /// ```
    /// # use async_trait::async_trait;
    /// # use std::net::SocketAddr;
    /// # use std::sync::Arc;
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::errors::TranslationError;
    /// # use scylla::policies::address_translator::{AddressTranslator, UntranslatedPeer};
    /// struct IdentityTranslator;
    ///
    /// #[async_trait]
    /// impl AddressTranslator for IdentityTranslator {
    ///     async fn translate_address(
    ///         &self,
    ///         untranslated_peer: &UntranslatedPeer,
    ///     ) -> Result<SocketAddr, TranslationError> {
    ///         Ok(untranslated_peer.untranslated_address())
    ///     }
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .address_translator(Arc::new(IdentityTranslator))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    /// # Example
    /// ```
    /// # use std::net::SocketAddr;
    /// # use std::sync::Arc;
    /// # use std::collections::HashMap;
    /// # use std::str::FromStr;
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::errors::TranslationError;
    /// # use scylla::policies::address_translator::AddressTranslator;
    /// #
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut translation_rules = HashMap::new();
    /// let addr_before_translation = SocketAddr::from_str("192.168.0.42:19042").unwrap();
    /// let addr_after_translation = SocketAddr::from_str("157.123.12.42:23203").unwrap();
    /// translation_rules.insert(addr_before_translation, addr_after_translation);
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .address_translator(Arc::new(translation_rules))
    ///     .build()
    ///     .await?;
    /// #    Ok(())
    /// # }
    /// ```
    pub fn address_translator(mut self, translator: Arc<dyn AddressTranslator>) -> Self {
        self.config.address_translator = Some(translator);
        self
    }

    /// TLS feature
    ///
    /// Provide SessionBuilder with TlsContext that will be
    /// used to create a TLS connection to the database.
    /// If set to None TLS connection won't be used.
    ///
    /// Default is None.
    ///
    #[cfg_attr(
        feature = "openssl-010",
        doc = r#"
# Example

```
    # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::PathBuf;
    use scylla::client::session::Session;
    use scylla::client::session_builder::SessionBuilder;
    use openssl::ssl::{SslContextBuilder, SslVerifyMode, SslMethod, SslFiletype};

    let certdir = fs::canonicalize(PathBuf::from("./examples/certs/scylla.crt"))?;
    let mut context_builder = SslContextBuilder::new(SslMethod::tls())?;
    context_builder.set_certificate_file(certdir.as_path(), SslFiletype::PEM)?;
    context_builder.set_verify(SslVerifyMode::NONE);

    let session: Session = SessionBuilder::new()
        .known_node("127.0.0.1:9042")
        .tls_context(Some(context_builder.build()))
        .build()
        .await?;
    # Ok(())
    # }
```
"#
    )]
    pub fn tls_context(mut self, tls_context: Option<impl Into<TlsContext>>) -> Self {
        self.config.tls_context = tls_context.map(|t| t.into());
        self
    }
}

// NOTE: this `impl` block contains configuration options specific for **Cloud** [`Session`].
// This means that if an option fits both non-Cloud and Cloud `Session`s, it should NOT be put
// here, but rather in `impl<K> GenericSessionBuilder<K>` block.
#[cfg(feature = "unstable-cloud")]
impl CloudSessionBuilder {
    /// Creates a new SessionBuilder with default configuration, based
    /// on the provided [`CloudConfig`].
    pub fn from_config(cloud_config: CloudConfig) -> Self {
        let mut config = SessionConfig::new();
        let mut exec_profile_builder = ExecutionProfile::builder();
        if let Some(default_consistency) = cloud_config.get_default_consistency() {
            exec_profile_builder = exec_profile_builder.consistency(default_consistency);
        }
        if let Some(default_serial_consistency) = cloud_config.get_default_serial_consistency() {
            exec_profile_builder =
                exec_profile_builder.serial_consistency(Some(default_serial_consistency));
        }
        config.default_execution_profile_handle = exec_profile_builder.build().into_handle();
        config.cloud_config = Some(Arc::new(cloud_config));
        CloudSessionBuilder {
            config,
            kind: PhantomData,
        }
    }

    /// Creates a new SessionBuilder with default configuration,
    /// based on provided path to Scylla Cloud Config yaml.
    pub fn new(
        cloud_config: impl AsRef<Path>,
        tls_provider: CloudTlsProvider,
    ) -> Result<Self, CloudConfigError> {
        let cloud_config = CloudConfig::read_from_yaml(cloud_config, tls_provider)?;
        Ok(Self::from_config(cloud_config))
    }
}

// This block contains configuration options that make sense both for Cloud and non-Cloud
// `Session`s. If an option fit only one of them, it should be put in a specialised block.
impl<K: SessionBuilderKind> GenericSessionBuilder<K> {
    /// Sets the local ip address all TCP sockets are bound to.
    ///
    /// By default, this option is set to `None`, which is equivalent to:
    /// - `INADDR_ANY` for IPv4 ([`Ipv4Addr::UNSPECIFIED`][std::net::Ipv4Addr::UNSPECIFIED])
    /// - `in6addr_any` for IPv6 ([`Ipv6Addr::UNSPECIFIED`][std::net::Ipv6Addr::UNSPECIFIED])
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::net::Ipv4Addr;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .local_ip_address(Some(Ipv4Addr::new(192, 168, 0, 1)))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn local_ip_address(mut self, local_ip_address: Option<impl Into<IpAddr>>) -> Self {
        self.config.local_ip_address = local_ip_address.map(Into::into);
        self
    }

    /// Specifies the local port range used for shard-aware connections.
    ///
    /// A possible use case is when you want to have multiple [`Session`] objects and do not want
    /// them to compete for the ports within the same range. It is then advised to assign
    /// mutually non-overlapping port ranges to each session object.
    ///
    /// By default this option is set to [`ShardAwarePortRange::EPHEMERAL_PORT_RANGE`].
    ///
    /// For details, see [`ShardAwarePortRange`] documentation.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::routing::ShardAwarePortRange;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .shard_aware_local_port_range(ShardAwarePortRange::new(49200..=50000)?)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn shard_aware_local_port_range(mut self, port_range: ShardAwarePortRange) -> Self {
        self.config.shard_aware_local_port_range = port_range;
        self
    }

    /// Set preferred Compression algorithm.
    /// The default is no compression.
    /// If it is not supported by database server Session will fall back to no encryption.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::client::Compression;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .compression(Some(Compression::Snappy))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn compression(mut self, compression: Option<Compression>) -> Self {
        self.config.compression = compression;
        self
    }

    /// Set the delay for schema agreement check. How often driver should ask if schema is in agreement
    /// The default is 200 milliseconds.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .schema_agreement_interval(Duration::from_secs(5))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn schema_agreement_interval(mut self, timeout: Duration) -> Self {
        self.config.schema_agreement_interval = timeout;
        self
    }

    /// Set the default execution profile using its handle
    ///
    /// # Example
    /// ```
    /// # use scylla::statement::Consistency;
    /// # use scylla::client::execution_profile::ExecutionProfile;
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let execution_profile = ExecutionProfile::builder()
    ///     .consistency(Consistency::All)
    ///     .request_timeout(Some(Duration::from_secs(2)))
    ///     .build();
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .default_execution_profile_handle(execution_profile.into_handle())
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn default_execution_profile_handle(
        mut self,
        profile_handle: ExecutionProfileHandle,
    ) -> Self {
        self.config.default_execution_profile_handle = profile_handle;
        self
    }

    /// Set the nodelay TCP flag.
    /// The default is true.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .tcp_nodelay(true)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn tcp_nodelay(mut self, nodelay: bool) -> Self {
        self.config.tcp_nodelay = nodelay;
        self
    }

    /// Set the TCP keepalive interval.
    /// The default is `None`, which implies that no keepalive messages
    /// are sent **on TCP layer** when a connection is idle.
    /// Note: CQL-layer keepalives are configured separately,
    /// with `Self::keepalive_interval`.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .tcp_keepalive_interval(std::time::Duration::from_secs(42))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn tcp_keepalive_interval(mut self, interval: Duration) -> Self {
        if interval <= Duration::from_secs(1) {
            warn!(
                "Setting the TCP keepalive interval to low values ({:?}) is not recommended as it can have a negative impact on performance. Consider setting it above 1 second.",
                interval
            );
        }

        self.config.tcp_keepalive_interval = Some(interval);
        self
    }

    /// Set keyspace to be used on all connections.\
    /// Each connection will send `"USE <keyspace_name>"` before sending any requests.\
    /// This can be later changed with [`crate::client::session::Session::use_keyspace`]
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::client::Compression;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .use_keyspace("my_keyspace_name", false)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn use_keyspace(mut self, keyspace_name: impl Into<String>, case_sensitive: bool) -> Self {
        self.config.used_keyspace = Some(keyspace_name.into());
        self.config.keyspace_case_sensitive = case_sensitive;
        self
    }

    /// Builds the Session after setting all the options.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::client::Compression;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .compression(Some(Compression::Snappy))
    ///     .build() // Turns SessionBuilder into Session
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn build(&self) -> Result<Session, NewSessionError> {
        Session::connect(self.config.clone()).await
    }

    /// Changes connection timeout
    /// The default is 5 seconds.
    /// If it's higher than underlying os's default connection timeout it won't effect.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .connection_timeout(Duration::from_secs(30))
    ///     .build() // Turns SessionBuilder into Session
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn connection_timeout(mut self, duration: Duration) -> Self {
        self.config.connect_timeout = duration;
        self
    }

    /// Sets the per-node connection pool size.
    /// The default is one connection per shard, which is the recommended setting for Scylla.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// use std::num::NonZeroUsize;
    /// use scylla::client::PoolSize;
    ///
    /// // This session will establish 4 connections to each node.
    /// // For ScyllaDB clusters, this number will be divided across shards
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .pool_size(PoolSize::PerHost(NonZeroUsize::new(4).unwrap()))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn pool_size(mut self, size: PoolSize) -> Self {
        self.config.connection_pool_size = size;
        self
    }

    /// If true, prevents the driver from connecting to the shard-aware port, even if the node supports it.
    ///
    /// _This is a Scylla-specific option_. It has no effect on Cassandra clusters.
    ///
    /// By default, connecting to the shard-aware port is __allowed__ and, in general, this setting
    /// _should not be changed_. The shard-aware port (19042 or 19142) makes the process of
    /// establishing connection per shard more robust compared to the regular transport port
    /// (9042 or 9142). With the shard-aware port, the driver is able to choose which shard
    /// will be assigned to the connection.
    ///
    /// In order to be able to use the shard-aware port effectively, the port needs to be
    /// reachable and not behind a NAT which changes source ports (the driver uses the source port
    /// to tell ScyllaDB which shard to assign). However, the driver is designed to behave in a robust
    /// way if those conditions are not met - if the driver fails to connect to the port or gets
    /// a connection to the wrong shard, it will re-attempt the connection to the regular transport port.
    ///
    /// The only cost of misconfigured shard-aware port should be a slightly longer reconnection time.
    /// If it is unacceptable to you or suspect that it causes you some other problems,
    /// you can use this option to disable the shard-aware port feature completely.
    /// However, __you should use it as a last resort__. Before you do that, we strongly recommend
    /// that you consider fixing the network issues.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .disallow_shard_aware_port(true)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn disallow_shard_aware_port(mut self, disallow: bool) -> Self {
        self.config.disallow_shard_aware_port = disallow;
        self
    }

    /// Set the timestamp generator that will generate timestamps on the client-side.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::policies::timestamp_generator::SimpleTimestampGenerator;
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .timestamp_generator(Arc::new(SimpleTimestampGenerator::new()))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn timestamp_generator(mut self, timestamp_generator: Arc<dyn TimestampGenerator>) -> Self {
        self.config.timestamp_generator = Some(timestamp_generator);
        self
    }

    /// Set the keyspaces to be fetched, to retrieve their strategy, and schema metadata if enabled
    /// No keyspaces, the default value, means all the keyspaces will be fetched.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .keyspaces_to_fetch(["my_keyspace"])
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn keyspaces_to_fetch(
        mut self,
        keyspaces: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.config.keyspaces_to_fetch = keyspaces.into_iter().map(Into::into).collect();
        self
    }

    /// Set the fetch schema metadata flag.
    /// The default is true.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .fetch_schema_metadata(true)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn fetch_schema_metadata(mut self, fetch: bool) -> Self {
        self.config.fetch_schema_metadata = fetch;
        self
    }

    /// Set the server-side timeout for metadata queries.
    /// The default is `Some(Duration::from_secs(2))`. It means that
    /// the all metadata queries will be set the 2 seconds timeout
    /// no matter what timeout is set as a cluster default.
    /// This prevents timeouts of schema queries when the schema is large
    /// and the default timeout is configured as tight.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .metadata_request_serverside_timeout(std::time::Duration::from_secs(5))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn metadata_request_serverside_timeout(mut self, timeout: Duration) -> Self {
        self.config.metadata_request_serverside_timeout = Some(timeout);
        self
    }

    /// Set the keepalive interval.
    /// The default is `Some(Duration::from_secs(30))`, which corresponds
    /// to keepalive CQL messages being sent every 30 seconds.
    /// Note: this configures CQL-layer keepalives. See also:
    /// `Self::tcp_keepalive_interval`.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .keepalive_interval(std::time::Duration::from_secs(42))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn keepalive_interval(mut self, interval: Duration) -> Self {
        if interval <= Duration::from_secs(1) {
            warn!(
                "Setting the keepalive interval to low values ({:?}) is not recommended as it can have a negative impact on performance. Consider setting it above 1 second.",
                interval
            );
        }

        self.config.keepalive_interval = Some(interval);
        self
    }

    /// Set the keepalive timeout.
    /// The default is `Some(Duration::from_secs(30))`. It means that
    /// the connection will be closed if time between sending a keepalive
    /// and receiving a response to any keepalive (not necessarily the same -
    /// it may be one sent later) exceeds 30 seconds.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .keepalive_timeout(std::time::Duration::from_secs(42))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn keepalive_timeout(mut self, timeout: Duration) -> Self {
        if timeout <= Duration::from_secs(1) {
            warn!(
                "Setting the keepalive timeout to low values ({:?}) is not recommended as it may aggressively close connections. Consider setting it above 5 seconds.",
                timeout
            );
        }

        self.config.keepalive_timeout = Some(timeout);
        self
    }

    /// Sets the timeout for waiting for schema agreement.
    /// By default, the timeout is 60 seconds.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .schema_agreement_timeout(std::time::Duration::from_secs(120))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn schema_agreement_timeout(mut self, timeout: Duration) -> Self {
        self.config.schema_agreement_timeout = timeout;
        self
    }

    /// Controls automatic waiting for schema agreement after a schema-altering
    /// statement is sent. By default, it is enabled.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .auto_await_schema_agreement(false)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn auto_await_schema_agreement(mut self, enabled: bool) -> Self {
        self.config.schema_agreement_automatic_waiting = enabled;
        self
    }

    /// Sets the host filter. The host filter decides whether any connections
    /// should be opened to the node or not. The driver will also avoid
    /// those nodes when re-establishing the control connection.
    ///
    /// See the [host filter](crate::policies::host_filter) module for a list
    /// of pre-defined filters. It is also possible to provide a custom filter
    /// by implementing the HostFilter trait.
    ///
    /// # Example
    /// ```
    /// # use async_trait::async_trait;
    /// # use std::net::SocketAddr;
    /// # use std::sync::Arc;
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::errors::TranslationError;
    /// # use scylla::policies::address_translator::AddressTranslator;
    /// # use scylla::policies::host_filter::DcHostFilter;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // The session will only connect to nodes from "my-local-dc"
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .host_filter(Arc::new(DcHostFilter::new("my-local-dc".to_string())))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn host_filter(mut self, filter: Arc<dyn HostFilter>) -> Self {
        self.config.host_filter = Some(filter);
        self
    }

    /// Set the refresh metadata on schema agreement flag.
    /// The default is true.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .refresh_metadata_on_auto_schema_agreement(true)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn refresh_metadata_on_auto_schema_agreement(mut self, refresh_metadata: bool) -> Self {
        self.config.refresh_metadata_on_auto_schema_agreement = refresh_metadata;
        self
    }

    /// Set the number of attempts to fetch [TracingInfo](crate::observability::tracing::TracingInfo)
    /// in [`Session::get_tracing_info`](crate::client::session::Session::get_tracing_info).
    /// The default is 5 attempts.
    ///
    /// Tracing info might not be available immediately on queried node - that's why
    /// the driver performs a few attempts with sleeps in between.
    ///
    /// Cassandra users may want to increase this value - the default is good
    /// for Scylla, but Cassandra sometimes needs more time for the data to
    /// appear in tracing table.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::num::NonZeroU32;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .tracing_info_fetch_attempts(NonZeroU32::new(10).unwrap())
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn tracing_info_fetch_attempts(mut self, attempts: NonZeroU32) -> Self {
        self.config.tracing_info_fetch_attempts = attempts;
        self
    }

    /// Set the delay between attempts to fetch [TracingInfo](crate::observability::tracing::TracingInfo)
    /// in [`Session::get_tracing_info`](crate::client::session::Session::get_tracing_info).
    /// The default is 3 milliseconds.
    ///
    /// Tracing info might not be available immediately on queried node - that's why
    /// the driver performs a few attempts with sleeps in between.
    ///
    /// Cassandra users may want to increase this value - the default is good
    /// for Scylla, but Cassandra sometimes needs more time for the data to
    /// appear in tracing table.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .tracing_info_fetch_interval(Duration::from_millis(50))
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn tracing_info_fetch_interval(mut self, interval: Duration) -> Self {
        self.config.tracing_info_fetch_interval = interval;
        self
    }

    /// Set the consistency level of fetching [TracingInfo](crate::observability::tracing::TracingInfo)
    /// in [`Session::get_tracing_info`](crate::client::session::Session::get_tracing_info).
    /// The default is [`Consistency::One`].
    ///
    /// # Example
    /// ```
    /// # use scylla::statement::Consistency;
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .tracing_info_fetch_consistency(Consistency::One)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn tracing_info_fetch_consistency(mut self, consistency: Consistency) -> Self {
        self.config.tracing_info_fetch_consistency = consistency;
        self
    }

    /// If true, the driver will inject a delay controlled by [SessionBuilder::write_coalescing_delay()]
    /// before flushing data to the socket.
    /// This gives the driver an opportunity to collect more write requests
    /// and write them in a single syscall, increasing the efficiency.
    ///
    /// However, this optimization may worsen latency if the rate of requests
    /// issued by the application is low, but otherwise the application is
    /// heavily loaded with other tasks on the same tokio executor.
    /// Please do performance measurements before committing to disabling
    /// this option.
    ///
    /// This option is true by default.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::client::Compression;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .write_coalescing(false) // Enabled by default
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn write_coalescing(mut self, enable: bool) -> Self {
        self.config.enable_write_coalescing = enable;
        self
    }

    /// Controls the write coalescing delay (if enabled).
    ///
    /// This option has no effect if [`SessionBuilder::write_coalescing()`] is set to false.
    ///
    /// This option is [`WriteCoalescingDelay::SmallNondeterministic`] by default.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::client::WriteCoalescingDelay;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let session: Session = SessionBuilder::new()
    ///     .known_node("127.0.0.1:9042")
    ///     .write_coalescing_delay(WriteCoalescingDelay::SmallNondeterministic)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn write_coalescing_delay(mut self, delay: WriteCoalescingDelay) -> Self {
        self.config.write_coalescing_delay = delay;
        self
    }

    /// Set the interval at which the driver refreshes the cluster metadata which contains information
    /// about the cluster topology as well as the cluster schema.
    ///
    /// The default is 60 seconds.
    ///
    /// In the given example, we have set the duration value to 20 seconds, which
    /// means that the metadata is refreshed every 20 seconds.
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    ///     let session: Session = SessionBuilder::new()
    ///         .known_node("127.0.0.1:9042")
    ///         .cluster_metadata_refresh_interval(std::time::Duration::from_secs(20))
    ///         .build()
    ///         .await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub fn cluster_metadata_refresh_interval(mut self, interval: Duration) -> Self {
        self.config.cluster_metadata_refresh_interval = interval;
        self
    }

    /// Set the custom identity of the driver/application/instance,
    /// to be sent as options in STARTUP message.
    ///
    /// By default driver name and version are sent;
    /// application name and version and client id are not sent.
    ///
    /// # Example
    /// ```
    /// # use scylla::client::session::Session;
    /// # use scylla::client::session_builder::SessionBuilder;
    /// # use scylla::client::SelfIdentity;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    ///     let (app_major, app_minor, app_patch) = (2, 1, 3);
    ///     let app_version = format!("{app_major}.{app_minor}.{app_patch}");
    ///
    ///     let session: Session = SessionBuilder::new()
    ///         .known_node("127.0.0.1:9042")
    ///         .custom_identity(
    ///             SelfIdentity::new()
    ///                 .with_custom_driver_version("0.13.0-custom_build_17")
    ///                 .with_application_name("my-app")
    ///                 .with_application_version(app_version)
    ///         )
    ///         .build()
    ///         .await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub fn custom_identity(mut self, identity: SelfIdentity<'static>) -> Self {
        self.config.identity = identity;
        self
    }
}

/// Creates a [`SessionBuilder`] with default configuration, same as [`SessionBuilder::new`]
impl Default for SessionBuilder {
    fn default() -> Self {
        SessionBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use scylla_cql::frame::types::SerialConsistency;
    use scylla_cql::Consistency;

    use super::super::Compression;
    use super::SessionBuilder;
    use crate::client::execution_profile::{defaults, ExecutionProfile};
    use crate::cluster::node::KnownNode;
    use crate::test_utils::setup_tracing;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    #[test]
    fn default_session_builder() {
        setup_tracing();
        let builder = SessionBuilder::new();

        assert!(builder.config.known_nodes.is_empty());
        assert_eq!(builder.config.compression, None);
    }

    #[test]
    fn add_known_node() {
        setup_tracing();
        let mut builder = SessionBuilder::new();

        builder = builder.known_node("test_hostname");

        assert_eq!(
            builder.config.known_nodes,
            vec![KnownNode::Hostname("test_hostname".into())]
        );
        assert_eq!(builder.config.compression, None);
    }

    #[test]
    fn add_known_node_addr() {
        setup_tracing();
        let mut builder = SessionBuilder::new();

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 3)), 1357);
        builder = builder.known_node_addr(addr);

        assert_eq!(builder.config.known_nodes, vec![KnownNode::Address(addr)]);
        assert_eq!(builder.config.compression, None);
    }

    #[test]
    fn add_known_nodes() {
        setup_tracing();
        let mut builder = SessionBuilder::new();

        builder = builder.known_nodes(["test_hostname1", "test_hostname2"]);

        assert_eq!(
            builder.config.known_nodes,
            vec![
                KnownNode::Hostname("test_hostname1".into()),
                KnownNode::Hostname("test_hostname2".into())
            ]
        );
        assert_eq!(builder.config.compression, None);
    }

    #[test]
    fn add_known_nodes_addr() {
        setup_tracing();
        let mut builder = SessionBuilder::new();

        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 3)), 1357);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 4)), 9090);

        builder = builder.known_nodes_addr([addr1, addr2]);

        assert_eq!(
            builder.config.known_nodes,
            vec![KnownNode::Address(addr1), KnownNode::Address(addr2)]
        );
        assert_eq!(builder.config.compression, None);
    }

    #[test]
    fn compression() {
        setup_tracing();
        let mut builder = SessionBuilder::new();
        assert_eq!(builder.config.compression, None);

        builder = builder.compression(Some(Compression::Lz4));
        assert_eq!(builder.config.compression, Some(Compression::Lz4));

        builder = builder.compression(Some(Compression::Snappy));
        assert_eq!(builder.config.compression, Some(Compression::Snappy));

        builder = builder.compression(None);
        assert_eq!(builder.config.compression, None);
    }

    #[test]
    fn tcp_nodelay() {
        setup_tracing();
        let mut builder = SessionBuilder::new();
        assert!(builder.config.tcp_nodelay);

        builder = builder.tcp_nodelay(false);
        assert!(!builder.config.tcp_nodelay);

        builder = builder.tcp_nodelay(true);
        assert!(builder.config.tcp_nodelay);
    }

    #[test]
    fn use_keyspace() {
        setup_tracing();
        let mut builder = SessionBuilder::new();
        assert_eq!(builder.config.used_keyspace, None);
        assert!(!builder.config.keyspace_case_sensitive);

        builder = builder.use_keyspace("ks_name_1", true);
        assert_eq!(builder.config.used_keyspace, Some("ks_name_1".to_string()));
        assert!(builder.config.keyspace_case_sensitive);

        builder = builder.use_keyspace("ks_name_2", false);
        assert_eq!(builder.config.used_keyspace, Some("ks_name_2".to_string()));
        assert!(!builder.config.keyspace_case_sensitive);
    }

    #[test]
    fn connection_timeout() {
        setup_tracing();
        let mut builder = SessionBuilder::new();
        assert_eq!(
            builder.config.connect_timeout,
            std::time::Duration::from_secs(5)
        );

        builder = builder.connection_timeout(std::time::Duration::from_secs(10));
        assert_eq!(
            builder.config.connect_timeout,
            std::time::Duration::from_secs(10)
        );
    }

    #[test]
    fn fetch_schema_metadata() {
        setup_tracing();
        let mut builder = SessionBuilder::new();
        assert!(builder.config.fetch_schema_metadata);

        builder = builder.fetch_schema_metadata(false);
        assert!(!builder.config.fetch_schema_metadata);

        builder = builder.fetch_schema_metadata(true);
        assert!(builder.config.fetch_schema_metadata);
    }

    // LatencyAwarePolicy, which is used in the test, requires presence of Tokio runtime.
    #[tokio::test]
    async fn execution_profile() {
        setup_tracing();
        let default_builder = SessionBuilder::new();
        let default_execution_profile = default_builder
            .config
            .default_execution_profile_handle
            .access();
        assert_eq!(
            default_execution_profile.consistency,
            defaults::consistency()
        );
        assert_eq!(
            default_execution_profile.serial_consistency,
            defaults::serial_consistency()
        );
        assert_eq!(
            default_execution_profile.request_timeout,
            defaults::request_timeout()
        );
        assert_eq!(
            default_execution_profile.load_balancing_policy.name(),
            defaults::load_balancing_policy().name()
        );

        let custom_consistency = Consistency::Any;
        let custom_serial_consistency = Some(SerialConsistency::Serial);
        let custom_timeout = Some(Duration::from_secs(1));
        let execution_profile_handle = ExecutionProfile::builder()
            .consistency(custom_consistency)
            .serial_consistency(custom_serial_consistency)
            .request_timeout(custom_timeout)
            .build()
            .into_handle();
        let builder_with_profile =
            default_builder.default_execution_profile_handle(execution_profile_handle.clone());
        let execution_profile = execution_profile_handle.access();

        let profile_in_builder = builder_with_profile
            .config
            .default_execution_profile_handle
            .access();
        assert_eq!(
            profile_in_builder.consistency,
            execution_profile.consistency
        );
        assert_eq!(
            profile_in_builder.serial_consistency,
            execution_profile.serial_consistency
        );
        assert_eq!(
            profile_in_builder.request_timeout,
            execution_profile.request_timeout
        );
        assert_eq!(
            profile_in_builder.load_balancing_policy.name(),
            execution_profile.load_balancing_policy.name()
        );
    }

    #[test]
    fn cluster_metadata_refresh_interval() {
        setup_tracing();
        let builder = SessionBuilder::new();
        assert_eq!(
            builder.config.cluster_metadata_refresh_interval,
            std::time::Duration::from_secs(60)
        );
    }

    #[test]
    fn all_features() {
        setup_tracing();
        let mut builder = SessionBuilder::new();

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 3)), 8465);
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 3)), 1357);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 4)), 9090);

        builder = builder.known_node("hostname_test");
        builder = builder.known_node_addr(addr);
        builder = builder.known_nodes(["hostname_test1", "hostname_test2"]);
        builder = builder.known_nodes_addr([addr1, addr2]);
        builder = builder.compression(Some(Compression::Snappy));
        builder = builder.tcp_nodelay(true);
        builder = builder.use_keyspace("ks_name", true);
        builder = builder.fetch_schema_metadata(false);
        builder = builder.cluster_metadata_refresh_interval(Duration::from_secs(1));

        assert_eq!(
            builder.config.known_nodes,
            vec![
                KnownNode::Hostname("hostname_test".into()),
                KnownNode::Address(addr),
                KnownNode::Hostname("hostname_test1".into()),
                KnownNode::Hostname("hostname_test2".into()),
                KnownNode::Address(addr1),
                KnownNode::Address(addr2),
            ]
        );

        assert_eq!(builder.config.compression, Some(Compression::Snappy));
        assert!(builder.config.tcp_nodelay);
        assert_eq!(
            builder.config.cluster_metadata_refresh_interval,
            Duration::from_secs(1)
        );

        assert_eq!(builder.config.used_keyspace, Some("ks_name".to_string()));

        assert!(builder.config.keyspace_case_sensitive);
        assert!(!builder.config.fetch_schema_metadata);
    }

    // This is to assert that #705 does not break the API (i.e. it merely extends it).
    fn _check_known_nodes_compatibility(
        hostnames: &[impl AsRef<str>],
        host_addresses: &[SocketAddr],
    ) {
        let mut sb: SessionBuilder = SessionBuilder::new();
        sb = sb.known_nodes(hostnames);
        sb = sb.known_nodes_addr(host_addresses);

        let mut config = sb.config;
        config.add_known_nodes(hostnames);
        config.add_known_nodes_addr(host_addresses);
    }
}
