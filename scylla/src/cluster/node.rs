use itertools::Itertools;
use tokio::net::lookup_host;
use tracing::warn;
use uuid::Uuid;

use crate::errors::{ConnectionPoolError, UseKeyspaceError};
use crate::network::Connection;
use crate::network::VerifiedKeyspaceName;
use crate::network::{NodeConnectionPool, PoolConfig};
#[cfg(feature = "metrics")]
use crate::observability::metrics::Metrics;
/// Node represents a cluster node along with it's data and connections
use crate::routing::{Shard, Sharder};

use std::fmt::Display;
use std::io;
use std::net::IpAddr;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    hash::{Hash, Hasher},
    net::SocketAddr,
    sync::Arc,
};

use crate::cluster::metadata::{PeerEndpoint, UntranslatedEndpoint};

/// This enum is introduced to support address translation only upon opening a connection,
/// as well as to cope with a bug present in older Cassandra and ScyllaDB releases.
/// The bug involves misconfiguration of rpc_address and/or broadcast_rpc_address
/// in system.local to 0.0.0.0. Mitigation involves replacing the faulty address
/// with connection's address, but then that address must not be subject to `AddressTranslator`,
/// so we carry that information using this enum. Address translation is never performed
/// on `Untranslatable` variant.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeAddr {
    /// Fetched in Metadata with `query_peers()` (broadcast by a node itself).
    Translatable(SocketAddr),
    /// Built from control connection's address upon `query_peers()` in order to mitigate the bug described above.
    Untranslatable(SocketAddr),
}

impl NodeAddr {
    pub(crate) fn into_inner(self) -> SocketAddr {
        match self {
            NodeAddr::Translatable(addr) | NodeAddr::Untranslatable(addr) => addr,
        }
    }
    pub(crate) fn inner_mut(&mut self) -> &mut SocketAddr {
        match self {
            NodeAddr::Translatable(addr) | NodeAddr::Untranslatable(addr) => addr,
        }
    }
    /// IP address of the node.
    ///
    /// Keep in mind that this discards the information about whether the address is translatable or not.
    /// Don't be surprised if you get a `Translatable` address here and won't be able to reach a node using it,
    /// because the node might be reachable through a different address, which must be obtained by translation.
    pub fn ip(&self) -> IpAddr {
        self.into_inner().ip()
    }

    /// Port of the node.
    pub fn port(&self) -> u16 {
        self.into_inner().port()
    }
}

impl Display for NodeAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.into_inner())
    }
}

/// Node represents a cluster node along with its data and connections
///
/// Note: if a Node changes its broadcast address, then it is not longer
/// represented by the same instance of Node struct, but instead
/// a new instance is created (for implementation reasons).
#[derive(Debug)]
pub struct Node {
    /// Unique identifier of the node.
    pub host_id: Uuid,
    /// Address of the node, which is used to connect to it.
    /// This address is either the one broadcast by the node itself
    /// (`NodeAddr::Translatable`) or the one used to connect to it
    /// in the first place if it's a contact point (`NodeAddr::Untranslatable`).
    pub address: NodeAddr,
    /// Datacenter of the node, if known.
    pub datacenter: Option<String>,
    /// Rack of the node, if known.
    pub rack: Option<String>,

    /// Connection pool for this node.
    ///
    /// If the node is filtered out by the host filter, this will be [None].
    pool: Option<NodeConnectionPool>,

    // In unit tests Node objects are mocked, and don't have real connection
    // pools. We want DefaultPolicy to use is_connected to filter out nodes,
    // but it would mean that all nodes would be filtered out in unit tests.

    // This field allows using is_enabled as a result of is_connected. Tests can
    // utilize this to simulate node being connected.
    #[cfg(test)]
    enabled_as_connected: AtomicBool,
}

/// A way that Nodes are often passed and accessed in the driver's code.
pub type NodeRef<'a> = &'a Arc<Node>;

impl Node {
    /// Creates a new node which starts connecting in the background.
    pub(crate) fn new(
        peer: PeerEndpoint,
        pool_config: &PoolConfig,
        keyspace_name: Option<VerifiedKeyspaceName>,
        enabled: bool,
        #[cfg(feature = "metrics")] metrics: Arc<Metrics>,
    ) -> Self {
        let host_id = peer.host_id;
        let address = peer.address;
        let datacenter = peer.datacenter.clone();
        let rack = peer.rack.clone();

        // We aren't interested in the fact that the pool becomes empty, so we immediately drop the receiving part.
        let (pool_empty_notifier, _) = tokio::sync::broadcast::channel(1);
        let pool = enabled.then(|| {
            NodeConnectionPool::new(
                UntranslatedEndpoint::Peer(peer),
                pool_config,
                keyspace_name,
                pool_empty_notifier,
                #[cfg(feature = "metrics")]
                metrics,
            )
        });

        Node {
            host_id,
            address,
            datacenter,
            rack,
            pool,
            #[cfg(test)]
            enabled_as_connected: AtomicBool::new(false),
        }
    }

    /// Recreates a Node after it changes its IP, preserving the pool.
    ///
    /// All settings except address are inherited from `node`.
    /// The underlying pool is preserved and notified about the IP change.
    /// # Arguments
    ///
    /// - `node` - previous definition of that node
    /// - `address` - new address to connect to
    pub(crate) fn inherit_with_ip_changed(node: &Node, endpoint: PeerEndpoint) -> Self {
        let address = endpoint.address;
        if let Some(ref pool) = node.pool {
            pool.update_endpoint(endpoint);
        }
        Self {
            address,
            datacenter: node.datacenter.clone(),
            rack: node.rack.clone(),
            host_id: node.host_id,
            pool: node.pool.clone(),
            #[cfg(test)]
            enabled_as_connected: AtomicBool::new(node.enabled_as_connected.load(Ordering::SeqCst)),
        }
    }

    /// Retrieves the sharder for this node, if it has one.
    ///
    /// If the node is disabled (i.e., it has no connection pool),
    /// or the node is not sharded (i.e., it's not a ScyllaDB node), this will return `None`.
    ///
    /// If the node [is enabled](Self::is_enabled) and does not have a sharder,
    /// this means it's not a ScyllaDB node.
    pub fn sharder(&self) -> Option<Sharder> {
        self.pool.as_ref()?.sharder()
    }

    /// Get a connection targetting the given shard
    /// If such connection is broken, get any random connection to this `Node`
    pub(crate) async fn connection_for_shard(
        &self,
        shard: Shard,
    ) -> Result<Arc<Connection>, ConnectionPoolError> {
        self.get_pool()?.connection_for_shard(shard)
    }

    /// Returns true if the driver has any open connections in the pool for this
    /// node.
    pub fn is_connected(&self) -> bool {
        #[cfg(test)]
        if self.enabled_as_connected.load(Ordering::SeqCst) {
            return self.is_enabled();
        }
        let Ok(pool) = self.get_pool() else {
            return false;
        };
        pool.is_connected()
    }

    /// Returns a boolean which indicates whether this node was is enabled.
    /// Only enabled nodes will have connections open. For disabled nodes,
    /// no connections will be opened.
    pub fn is_enabled(&self) -> bool {
        self.pool.is_some()
    }

    pub(crate) async fn use_keyspace(
        &self,
        keyspace_name: VerifiedKeyspaceName,
    ) -> Result<(), UseKeyspaceError> {
        if let Some(pool) = &self.pool {
            pool.use_keyspace(keyspace_name).await?;
        }
        Ok(())
    }

    pub(crate) fn get_working_connections(
        &self,
    ) -> Result<Vec<Arc<Connection>>, ConnectionPoolError> {
        self.get_pool()?.get_working_connections()
    }

    pub(crate) fn get_random_connection(&self) -> Result<Arc<Connection>, ConnectionPoolError> {
        self.get_pool()?.random_connection()
    }

    pub(crate) async fn wait_until_pool_initialized(&self) {
        if let Some(pool) = &self.pool {
            pool.wait_until_initialized().await;
        }
    }

    fn get_pool(&self) -> Result<&NodeConnectionPool, ConnectionPoolError> {
        self.pool
            .as_ref()
            .ok_or(ConnectionPoolError::NodeDisabledByHostFilter)
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.host_id == other.host_id
    }
}

impl Eq for Node {}

impl Hash for Node {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.host_id.hash(state);
    }
}

/// Describes a database server known on `Session` startup.
///
/// The name derives from SessionBuilder's `known_node()` family of methods.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[non_exhaustive]
pub enum KnownNode {
    /// A node identified by its hostname.
    Hostname(String),
    /// A node identified by its IP address + a port.
    Address(SocketAddr),
}

/// Describes a database server known on `Session` startup.
/// It is similar to [KnownNode] but includes also `CloudEndpoint` variant,
/// which is created by the driver if session is configured to connect to
/// serverless cluster.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum InternalKnownNode {
    Hostname(String),
    Address(SocketAddr),
    #[cfg(feature = "unstable-cloud")]
    CloudEndpoint(CloudEndpoint),
}

impl From<KnownNode> for InternalKnownNode {
    fn from(value: KnownNode) -> Self {
        match value {
            KnownNode::Hostname(s) => InternalKnownNode::Hostname(s),
            KnownNode::Address(s) => InternalKnownNode::Address(s),
        }
    }
}

/// Describes a database server in the serverless Scylla Cloud.
#[cfg(feature = "unstable-cloud")]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct CloudEndpoint {
    pub(crate) hostname: String,
    pub(crate) datacenter: String,
}

/// Describes a database server known on Session startup, with already resolved address.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedContactPoint {
    pub(crate) address: SocketAddr,
    #[cfg_attr(not(feature = "unstable-cloud"), expect(unused))]
    pub(crate) datacenter: Option<String>,
}

// Resolve the given hostname using a DNS lookup if necessary.
// The resolution may return multiple IPs and the function returns one of them.
// It prefers to return IPv4s first, and only if there are none, IPv6s.
pub(crate) async fn resolve_hostname(hostname: &str) -> Result<SocketAddr, io::Error> {
    let addrs = match lookup_host(hostname).await {
        Ok(addrs) => itertools::Either::Left(addrs),
        // Use a default port in case of error, but propagate the original error on failure
        Err(e) => {
            let addrs = lookup_host((hostname, 9042)).await.or(Err(e))?;
            itertools::Either::Right(addrs)
        }
    };

    addrs
        .find_or_last(|addr| matches!(addr, SocketAddr::V4(_)))
        .ok_or_else(|| {
            io::Error::other(format!("Empty address list returned by DNS for {hostname}"))
        })
}

/// Transforms the given [`InternalKnownNode`]s into [`ContactPoint`]s.
///
/// In case of a hostname, resolves it using a DNS lookup.
/// In case of a plain IP address, parses it and uses straight.
pub(crate) async fn resolve_contact_points(
    known_nodes: &[InternalKnownNode],
) -> (Vec<ResolvedContactPoint>, Vec<String>) {
    // Find IP addresses of all known nodes passed in the config
    let mut initial_peers: Vec<ResolvedContactPoint> = Vec::with_capacity(known_nodes.len());

    let mut to_resolve: Vec<(&String, Option<String>)> = Vec::new();
    let mut hostnames: Vec<String> = Vec::new();

    for node in known_nodes.iter() {
        match node {
            InternalKnownNode::Hostname(hostname) => {
                to_resolve.push((hostname, None));
                hostnames.push(hostname.clone());
            }
            InternalKnownNode::Address(address) => initial_peers.push(ResolvedContactPoint {
                address: *address,
                datacenter: None,
            }),
            #[cfg(feature = "unstable-cloud")]
            InternalKnownNode::CloudEndpoint(CloudEndpoint {
                hostname,
                datacenter,
            }) => to_resolve.push((hostname, Some(datacenter.clone()))),
        };
    }
    let resolve_futures = to_resolve
        .into_iter()
        .map(|(hostname, datacenter)| async move {
            match resolve_hostname(hostname).await {
                Ok(address) => Some(ResolvedContactPoint {
                    address,
                    datacenter,
                }),
                Err(e) => {
                    warn!("Hostname resolution failed for {}: {}", hostname, &e);
                    None
                }
            }
        });
    let resolved: Vec<_> = futures::future::join_all(resolve_futures).await;
    initial_peers.extend(resolved.into_iter().flatten());

    (initial_peers, hostnames)
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Node {
        pub(crate) fn new_for_test(
            id: Option<Uuid>,
            address: Option<NodeAddr>,
            datacenter: Option<String>,
            rack: Option<String>,
        ) -> Self {
            Self {
                host_id: id.unwrap_or(Uuid::new_v4()),
                address: address.unwrap_or(NodeAddr::Translatable(SocketAddr::from((
                    [255, 255, 255, 255],
                    0,
                )))),
                datacenter,
                rack,
                pool: None,
                enabled_as_connected: AtomicBool::new(false),
            }
        }

        pub(crate) fn use_enabled_as_connected(&self) {
            self.enabled_as_connected.store(true, Ordering::SeqCst);
        }
    }
}
