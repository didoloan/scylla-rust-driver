//! This module contains various errors which can be returned by [`Session`](crate::client::session::Session).

use std::error::Error;
use std::io::ErrorKind;
use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::num::ParseIntError;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

use crate::frame::response;

// Re-export error types from pager module.
pub use crate::client::pager::{NextPageError, NextRowError};

use crate::statement::prepared::TokenCalculationError;
// Re-export error types from query_result module.
pub use crate::response::query_result::{
    FirstRowError, IntoRowsResultError, MaybeFirstRowError, ResultNotRowsError, RowsError,
    SingleRowError,
};

// Re-export error type from authentication module.
pub use crate::authentication::AuthError;

// Re-export error type from network module.
pub use crate::network::tls::TlsError;

// Re-export error types from scylla-cql.
pub use scylla_cql::deserialize::{DeserializationError, TypeCheckError};
pub use scylla_cql::frame::frame_errors::{
    CqlAuthChallengeParseError, CqlAuthSuccessParseError, CqlAuthenticateParseError,
    CqlErrorParseError, CqlEventParseError, CqlRequestSerializationError, CqlResponseParseError,
    CqlResultParseError, CqlSupportedParseError, FrameBodyExtensionsParseError,
    FrameHeaderParseError,
};
pub use scylla_cql::frame::request::CqlRequestKind;
pub use scylla_cql::frame::response::error::{DbError, OperationType, WriteType};
pub use scylla_cql::frame::response::CqlResponseKind;
pub use scylla_cql::serialize::SerializationError;

/// Error that occurred during request execution
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum ExecutionError {
    /// Caller passed an invalid statement.
    #[error(transparent)]
    BadQuery(#[from] BadQuery),

    /// Load balancing policy returned an empty plan.
    #[error(
        "Load balancing policy returned an empty plan. \
        If you are using DefaultPolicy, ensure that you have not selected a nonexistent datacenter as preferred. \
        If you are using a custom LBP implementation, ensure that your LBP implementation is correct. \
        If neither of the above suggestions is the cause, then this is most likely a driver bug!"
    )]
    EmptyPlan,

    /// Failed to prepare the statement.
    /// Applies to unprepared statements with non-empty value parameters.
    #[error("Failed to prepare the statement: {0}")]
    PrepareError(#[from] PrepareError),

    /// Selected node's connection pool is in invalid state.
    #[error("No connections in the pool: {0}")]
    ConnectionPoolError(#[from] ConnectionPoolError),

    /// An error returned by last attempt of request execution.
    #[error(transparent)]
    LastAttemptError(#[from] RequestAttemptError),

    /// Failed to run a request within a provided client timeout.
    #[error(
        "Request execution exceeded a client timeout of {}ms",
        std::time::Duration::as_millis(.0)
    )]
    RequestTimeout(std::time::Duration),

    /// 'USE KEYSPACE <>' request failed.
    #[error("'USE KEYSPACE <>' request failed: {0}")]
    UseKeyspaceError(#[from] UseKeyspaceError),

    /// Failed to await automatic schema agreement.
    #[error("Failed to await schema agreement: {0}")]
    SchemaAgreementError(#[from] SchemaAgreementError),

    /// A metadata error occurred during schema agreement.
    #[error("Cluster metadata fetch error occurred during automatic schema agreement: {0}")]
    MetadataError(#[from] MetadataError),
}

impl From<SerializationError> for ExecutionError {
    fn from(serialized_err: SerializationError) -> ExecutionError {
        ExecutionError::BadQuery(BadQuery::SerializationError(serialized_err))
    }
}

/// An error returned by [`Session::prepare()`][crate::client::session::Session::prepare].
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum PrepareError {
    /// Failed to find a node with working connection pool.
    #[error("Failed to find a node with working connection pool: {0}")]
    ConnectionPoolError(#[from] ConnectionPoolError),

    /// Failed to prepare statement on every connection from the pool.
    #[error("Preparation failed on every connection from the selected pool. First attempt error: {first_attempt}")]
    AllAttemptsFailed {
        /// Error that the first attempt failed with.
        first_attempt: RequestAttemptError,
    },

    /// Prepared statement id mismatch.
    #[error(
        "Prepared statement id mismatch between multiple connections - all result ids should be equal."
    )]
    PreparedStatementIdsMismatch,
}

/// An error that occurred during construction of [`QueryPager`][crate::client::pager::QueryPager].
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
// Check triggers because all variants end with "Error".
// TODO(2.0): Remove the "Error" postfix from variants.
#[expect(clippy::enum_variant_names)]
pub enum PagerExecutionError {
    /// Failed to prepare the statement.
    #[error("Failed to prepare the statement to be used by the pager: {0}")]
    PrepareError(#[from] PrepareError),

    /// Failed to serialize statement parameters.
    #[error("Failed to serialize statement parameters: {0}")]
    SerializationError(#[from] SerializationError),

    /// Failed to fetch the first page of the result.
    #[error("Failed to fetch the first page of the result: {0}")]
    NextPageError(#[from] NextPageError),
}

/// Error that occurred during session creation
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum NewSessionError {
    /// Failed to resolve hostname passed in Session creation
    #[error("Couldn't resolve any hostname: {0:?}")]
    FailedToResolveAnyHostname(Vec<String>),

    /// List of known nodes passed to Session constructor is empty
    /// There needs to be at least one node to connect to
    #[error("Empty known nodes list")]
    EmptyKnownNodesList,

    /// Failed to perform initial cluster metadata fetch.
    #[error("Failed to perform initial cluster metadata fetch: {0}")]
    MetadataError(#[from] MetadataError),

    /// 'USE KEYSPACE <>' request failed.
    #[error("'USE KEYSPACE <>' request failed: {0}")]
    UseKeyspaceError(#[from] UseKeyspaceError),
}

/// An error that occurred during `USE KEYSPACE <>` request.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum UseKeyspaceError {
    /// Passed invalid keyspace name to use.
    #[error("Passed invalid keyspace name to use: {0}")]
    BadKeyspaceName(#[from] BadKeyspaceName),

    /// An error during request execution.
    #[error(transparent)]
    RequestError(#[from] RequestAttemptError),

    /// Keyspace name mismatch.
    #[error("Keyspace name mismatch; expected: {expected_keyspace_name_lowercase}, received: {result_keyspace_name_lowercase}")]
    KeyspaceNameMismatch {
        /// Expected keyspace name, in lowercase.
        expected_keyspace_name_lowercase: String,
        /// Received keyspace name, in lowercase.
        result_keyspace_name_lowercase: String,
    },

    /// Failed to run a request within a provided client timeout.
    #[error(
        "Request execution exceeded a client timeout of {}ms",
        std::time::Duration::as_millis(.0)
    )]
    RequestTimeout(std::time::Duration),
}

/// An error that occurred when awating schema agreement.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum SchemaAgreementError {
    /// Failed to find a node with working connection pool.
    #[error("Failed to find a node with working connection pool: {0}")]
    ConnectionPoolError(#[from] ConnectionPoolError),

    /// Failed to execute schema version query on one of the connections.
    ///
    /// The driver attempts to fetch schema version on all connections in the pool (for all nodes).
    /// It expects all of the requests to succeed. If at least one request fails, schema version
    /// fetch is considered failed. This variant contains an error from one of the failing request attempts.
    #[error("Failed to execute schema version query: {0}")]
    RequestError(#[from] RequestAttemptError),

    /// Failed to convert schema version query result into rows result.
    #[error("Failed to convert schema version query result into rows result: {0}")]
    TracesEventsIntoRowsResultError(IntoRowsResultError),

    /// Failed to deserialize a single row from schema version query response.
    #[error(transparent)]
    SingleRowError(SingleRowError),

    /// Schema agreement timed out.
    #[error("Schema agreement exceeded {}ms", std::time::Duration::as_millis(.0))]
    Timeout(std::time::Duration),

    /// Some host mandatory for schema agreement is not present in the connection pool.
    #[error(
        "Host with id {} required for schema agreement is not present in connection pool",
        0
    )]
    RequiredHostAbsent(Uuid),
}

/// An error that occurred during tracing info fetch.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum TracingError {
    /// Failed to execute query to either "system_traces.sessions" or "system_traces.events".
    #[error(
        "Failed to execute queries to \"system_traces.sessions\" or \"system_traces.events\" system tables: {0}"
    )]
    ExecutionError(#[from] ExecutionError),

    /// Failed to convert result of system_traces.session query to rows result.
    #[error("Failed to convert result of system_traces.session query to rows result")]
    TracesSessionIntoRowsResultError(IntoRowsResultError),

    /// system_traces.session has invalid column type.
    #[error("system_traces.session has invalid column type: {0}")]
    TracesSessionInvalidColumnType(TypeCheckError),

    /// Response to system_traces.session failed to deserialize.
    #[error("Response to system_traces.session failed to deserialize: {0}")]
    TracesSessionDeserializationFailed(DeserializationError),

    /// Failed to convert result of system_traces.events query to rows result.
    #[error("Failed to convert result of system_traces.events query to rows result")]
    TracesEventsIntoRowsResultError(IntoRowsResultError),

    /// system_traces.events has invalid column type.
    #[error("system_traces.events has invalid column type: {0}")]
    TracesEventsInvalidColumnType(TypeCheckError),

    /// Response to system_traces.events failed to deserialize.
    #[error("Response to system_traces.events failed to deserialize: {0}")]
    TracesEventsDeserializationFailed(DeserializationError),

    /// All tracing queries returned an empty result.
    #[error(
        "All tracing queries returned an empty result, \
        maybe the trace information didn't propagate yet. \
        Consider configuring Session with \
        a longer fetch interval (tracing_info_fetch_interval)"
    )]
    EmptyResults,
}

/// An error that occurred during metadata fetch and verification.
///
/// The driver performs metadata fetch and verification of the cluster's schema
/// and topology. This includes:
/// - keyspaces
/// - UDTs
/// - tables
/// - views
/// - peers (topology)
///
/// The errors that occur during metadata fetch are contained in [`MetadataFetchError`].
/// Remaining errors (logical errors) are contained in the variants corresponding to the
/// specific part of the metadata.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum MetadataError {
    /// Control connection pool error.
    #[error("Control connection pool error: {0}")]
    ConnectionPoolError(#[from] ConnectionPoolError),

    /// Failed to fetch metadata.
    #[error("transparent")]
    FetchError(#[from] MetadataFetchError),

    /// Bad peers metadata.
    #[error("Bad peers metadata: {0}")]
    Peers(#[from] PeersMetadataError),

    /// Bad keyspaces metadata.
    #[error("Bad keyspaces metadata: {0}")]
    Keyspaces(#[from] KeyspacesMetadataError),

    /// Bad UDTs metadata.
    #[error("Bad UDTs metadata: {0}")]
    Udts(#[from] UdtMetadataError),

    /// Bad tables metadata.
    #[error("Bad tables metadata: {0}")]
    Tables(#[from] TablesMetadataError),
}

/// An error occurred during metadata fetch.
#[derive(Error, Debug, Clone)]
#[error("Metadata fetch failed for table \"{table}\": {error}")]
#[non_exhaustive]
pub struct MetadataFetchError {
    /// Reason why metadata fetch failed.
    pub error: MetadataFetchErrorKind,
    /// Table name for which metadata fetch failed.
    pub table: &'static str,
}

/// Specific reason why metadata fetch failed.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum MetadataFetchErrorKind {
    /// Queried table has invalid column type.
    #[error("The table has invalid column type: {0}")]
    InvalidColumnType(#[from] TypeCheckError),

    /// Failed to prepare the statement for metadata fetch.
    #[error("Failed to prepare the statement: {0}")]
    PrepareError(#[from] RequestAttemptError),

    /// Failed to serialize statement parameters.
    #[error("Failed to serialize statement parameters: {0}")]
    SerializationError(#[from] SerializationError),

    /// Failed to obtain next row from response to the metadata fetch query.
    #[error("Failed to obtain next row from response to the query: {0}")]
    NextRowError(#[from] NextRowError),
}

/// An error that occurred during peers metadata fetch.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum PeersMetadataError {
    /// Empty peers list returned during peers metadata fetch.
    #[error("Peers list is empty")]
    EmptyPeers,

    /// All peers have empty token lists.
    #[error("All peers have empty token lists")]
    EmptyTokenLists,
}

/// An error that occurred during keyspaces metadata fetch.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum KeyspacesMetadataError {
    /// Bad keyspace replication strategy.
    #[error("Bad keyspace <{keyspace}> replication strategy: {error}")]
    Strategy {
        /// Keyspace name for which the error occurred.
        keyspace: String,
        /// Reason why the keyspace strategy is bad.
        error: KeyspaceStrategyError,
    },
}

/// An error that occurred during specific keyspace's metadata fetch.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum KeyspaceStrategyError {
    /// Keyspace strategy map missing a `class` field.
    #[error("keyspace strategy definition is missing a 'class' field")]
    MissingClassForStrategyDefinition,

    /// Missing replication factor for SimpleStrategy.
    #[error("Missing replication factor field for SimpleStrategy")]
    MissingReplicationFactorForSimpleStrategy,

    /// Replication factor could not be parsed as unsigned integer.
    #[error("Failed to parse a replication factor as unsigned integer: {0}")]
    ReplicationFactorParseError(ParseIntError),

    /// Received an unexpected NTS option.
    /// Driver expects only 'class' and replication factor per dc ('dc': rf)
    #[error("Unexpected NetworkTopologyStrategy option: '{key}': '{value}'")]
    UnexpectedNetworkTopologyStrategyOption {
        /// The key of the unexpected option entry.
        key: String,
        /// The value of the unexpected option entry.
        value: String,
    },
}

/// An error that occurred during UDTs metadata fetch.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum UdtMetadataError {
    /// Failed to parse CQL type returned from system_schema.types query.
    #[error(
        "Failed to parse a CQL type returned from system_schema.types query. \
        Type '{typ}', at position {position}: {reason}"
    )]
    InvalidCqlType {
        /// (Invalid) name of the invalid CQL type.
        typ: String,
        /// Position in the CQL type string where the error occurred.
        position: usize,
        /// Reason why the CQL type name is invalid.
        reason: String,
    },

    /// Circular UDT dependency detected.
    #[error("Detected circular dependency between user defined types - toposort is impossible!")]
    CircularTypeDependency,
}

/// An error that occurred during tables metadata fetch.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum TablesMetadataError {
    /// Failed to parse CQL type returned from system_schema.columns query.
    #[error(
        "Failed to parse a CQL type returned from system_schema.columns query. \
        Type '{typ}', at position {position}: {reason}"
    )]
    InvalidCqlType {
        /// (Invalid) name of the invalid CQL type.
        typ: String,
        /// Position in the CQL type string where the error occurred.
        position: usize,
        /// Reason why the CQL type name is invalid.
        reason: String,
    },

    /// Unknown column kind.
    #[error("Unknown column kind '{column_kind}' for {keyspace_name}.{table_name}.{column_name}")]
    UnknownColumnKind {
        /// Keyspace name where the error occurred.
        keyspace_name: String,
        /// Table name where the error occurred.
        table_name: String,
        /// Column name where the error occurred.
        column_name: String,
        /// Kind of the column that is unknown.
        column_kind: String,
    },
}

/// Error caused by caller creating an invalid statement.
#[derive(Error, Debug, Clone)]
#[error("Invalid statement passed to Session")]
#[non_exhaustive]
pub enum BadQuery {
    /// Unable extract a partition key based on prepared statement's metadata.
    #[error("Unable extract a partition key based on prepared statement's metadata")]
    PartitionKeyExtraction,

    /// "Serializing values failed.
    #[error("Serializing values failed: {0} ")]
    SerializationError(#[from] SerializationError),

    /// Serialized values are too long to compute partition key.
    #[error("Serialized values are too long to compute partition key! Length: {0}, Max allowed length: {1}")]
    ValuesTooLongForKey(usize, usize),

    /// Too many statements in the batch statement.
    #[error("Number of statements in Batch Statement supplied is {0} which has exceeded the max value of 65,535")]
    TooManyQueriesInBatchStatement(usize),
}

/// Invalid keyspace name given to `Session::use_keyspace()`
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum BadKeyspaceName {
    /// Keyspace name is empty
    #[error("Keyspace name is empty")]
    Empty,

    /// Keyspace name too long, must be up to 48 characters
    #[error("Keyspace name too long, must be up to 48 characters, found {1} characters. Bad keyspace name: '{0}'")]
    TooLong(String, usize),

    /// Illegal character - only alphanumeric and underscores allowed.
    #[error("Illegal character found: '{1}', only alphanumeric and underscores allowed. Bad keyspace name: '{0}'")]
    IllegalCharacter(String, char),
}

/// An error that occurred when selecting a node connection
/// to perform a request on.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum ConnectionPoolError {
    /// A connection pool is broken. Includes an error of a last connection.
    #[error("The pool is broken; Last connection failed with: {last_connection_error}")]
    Broken {
        /// The error that the last connection attempt failed with.
        last_connection_error: ConnectionError,
    },

    /// A connection pool is still being initialized.
    #[error("Pool is still being initialized")]
    Initializing,

    /// A corresponding node was disabled by a host filter.
    #[error("The node has been disabled by a host filter")]
    NodeDisabledByHostFilter,
}

/// An error that appeared on a connection level.
/// It indicated that connection can no longer be used
/// and should be dropped.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum ConnectionError {
    /// Provided connect timeout elapsed.
    #[error("Connect timeout elapsed")]
    ConnectTimeout,

    /// Input/Output error occurred.
    #[error(transparent)]
    IoError(Arc<std::io::Error>),

    /// Driver was unable to find a free source port for given shard.
    #[error("Could not find free source port for shard {0}")]
    NoSourcePortForShard(u32),

    /// Failed to translate an address before establishing a connection.
    #[error("Address translation failed: {0}")]
    TranslationError(#[from] TranslationError),

    /// A connection has been broken after being established.
    #[error(transparent)]
    BrokenConnection(#[from] BrokenConnectionError),

    /// A request required to initialize a connection failed.
    #[error(transparent)]
    ConnectionSetupRequestError(#[from] ConnectionSetupRequestError),
}

impl From<std::io::Error> for ConnectionError {
    fn from(value: std::io::Error) -> Self {
        ConnectionError::IoError(Arc::new(value))
    }
}

impl ConnectionError {
    /// Checks if this error indicates that a chosen source port/address cannot be bound.
    /// This is caused by one of the following:
    /// - The source address is already used by another socket,
    /// - The source address is reserved and the process does not have sufficient privileges to use it.
    pub fn is_address_unavailable_for_use(&self) -> bool {
        if let ConnectionError::IoError(io_error) = self {
            match io_error.kind() {
                ErrorKind::AddrInUse | ErrorKind::PermissionDenied => return true,
                _ => {}
            }
        }

        false
    }
}

/// Error caused by failed address translation done before establishing connection
#[non_exhaustive]
#[derive(Debug, Clone, Error)]
pub enum TranslationError {
    /// Driver failed to find a translation rule for a provided address.
    #[error("No rule for address {0}")]
    NoRuleForAddress(SocketAddr),

    /// A translation rule for a provided address was found, but the translated address was invalid.
    #[error("Failed to parse translated address: {translated_addr_str}, reason: {reason}")]
    InvalidAddressInRule {
        /// The invalid translated address string.
        translated_addr_str: &'static str,
        /// Reason why the string is not a valid address.
        reason: AddrParseError,
    },

    /// An I/O error occurred during address translation.
    #[error("An I/O error occurred during address translation: {0}")]
    IoError(Arc<std::io::Error>),
}

/// An error that occurred during connection setup request execution.
/// It indicates that request needed to initiate a connection failed.
#[derive(Error, Debug, Clone)]
#[error("Failed to perform a connection setup request. Request: {request_kind}, reason: {error}")]
#[non_exhaustive]
pub struct ConnectionSetupRequestError {
    /// Kind of the request that failed.
    pub request_kind: CqlRequestKind,
    /// Reason why the request failed.
    pub error: ConnectionSetupRequestErrorKind,
}

/// Specific reason why a connection setup request failed.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum ConnectionSetupRequestErrorKind {
    /// Failed to serialize CQL request.
    #[error("Failed to serialize CQL request: {0}")]
    CqlRequestSerialization(#[from] CqlRequestSerializationError),

    /// Failed to deserialize frame body extensions.
    #[error(transparent)]
    BodyExtensionsParseError(#[from] FrameBodyExtensionsParseError),

    /// Driver was unable to allocate a stream id to execute a setup request on.
    #[error("Unable to allocate stream id")]
    UnableToAllocStreamId,

    /// A connection was broken during setup request execution.
    #[error(transparent)]
    BrokenConnection(#[from] BrokenConnectionError),

    /// Received a server error in response to connection setup request.
    #[error("Database returned an error: {0}, Error message: {1}")]
    DbError(DbError, String),

    /// Received an unexpected response from the server.
    #[error("Received unexpected response from the server: {0}")]
    UnexpectedResponse(CqlResponseKind),

    /// Received a response to OPTIONS request, but failed to deserialize its body.
    #[error("Failed to deserialize SUPPORTED response: {0}")]
    CqlSupportedParseError(#[from] CqlSupportedParseError),

    /// Received an AUTHENTICATE response, but failed to deserialize its body.
    #[error("Failed to deserialize AUTHENTICATE response: {0}")]
    CqlAuthenticateParseError(#[from] CqlAuthenticateParseError),

    /// Received an AUTH_SUCCESS response, but failed to deserialize its body.
    #[error("Failed to deserialize AUTH_SUCCESS response: {0}")]
    CqlAuthSuccessParseError(#[from] CqlAuthSuccessParseError),

    /// Received an AUTH_CHALLENGE response, but failed to deserialize its body.
    #[error("Failed to deserialize AUTH_CHALLENGE response: {0}")]
    CqlAuthChallengeParseError(#[from] CqlAuthChallengeParseError),

    /// Received server ERROR response, but failed to deserialize its body.
    #[error("Failed to deserialize ERROR response: {0}")]
    CqlErrorParseError(#[from] CqlErrorParseError),

    /// An error returned by [`AuthenticatorProvider::start_authentication_session`](crate::authentication::AuthenticatorProvider::start_authentication_session).
    #[error("Failed to start client's auth session: {0}")]
    StartAuthSessionError(AuthError),

    /// An error returned by [`AuthenticatorSession::evaluate_challenge`](crate::authentication::AuthenticatorSession::evaluate_challenge).
    #[error("Failed to evaluate auth challenge on client side: {0}")]
    AuthChallengeEvaluationError(AuthError),

    /// An error returned by [`AuthenticatorSession::success`](crate::authentication::AuthenticatorSession::success).
    #[error("Failed to finish auth challenge on client side: {0}")]
    AuthFinishError(AuthError),

    /// User did not provide authentication while the cluster requires it.
    /// See [`SessionBuilder::user`](crate::client::session_builder::SessionBuilder::user)
    /// and/or [`SessionBuilder::authenticator_provider`](crate::client::session_builder::SessionBuilder::authenticator_provider).
    #[error("Authentication is required. You can use SessionBuilder::user(\"user\", \"pass\") to provide credentials or SessionBuilder::authenticator_provider to provide custom authenticator")]
    MissingAuthentication,
}

impl ConnectionSetupRequestError {
    pub(crate) fn new(
        request_kind: CqlRequestKind,
        error: ConnectionSetupRequestErrorKind,
    ) -> Self {
        ConnectionSetupRequestError {
            request_kind,
            error,
        }
    }

    /// Retrieves the specific error that occurred during connection setup request execution.
    pub fn get_error(&self) -> &ConnectionSetupRequestErrorKind {
        &self.error
    }
}

/// An error indicating that a connection was broken.
/// Possible error reasons:
/// - keepalive query errors - driver failed to sent a keepalive query, or the query timed out
/// - received a frame with unexpected stream id
/// - failed to handle a server event (message received on stream -1)
/// - some low-level IO errors - e.g. driver failed to write data via socket
#[derive(Error, Debug, Clone)]
#[error("Connection broken, reason: {0}")]
pub struct BrokenConnectionError(Arc<dyn Error + Sync + Send>);

impl BrokenConnectionError {
    /// Retrieve an error reason by downcasting to specific type.
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

/// A reason why connection was broken.
///
/// See [`BrokenConnectionError::downcast_ref()`].
/// You can retrieve the actual type by downcasting `Arc<dyn Error>`.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum BrokenConnectionErrorKind {
    /// Driver sent a keepalive request to the database, but the request timed out.
    #[error("Timed out while waiting for response to keepalive request on connection to node {0}")]
    KeepaliveTimeout(IpAddr),

    /// Driver sent a keepalive request to the database, but request execution failed.
    #[error("Failed to execute keepalive request: {0}")]
    KeepaliveRequestError(Arc<dyn Error + Sync + Send>),

    /// Failed to deserialize response frame header.
    #[error("Failed to deserialize frame: {0}")]
    FrameHeaderParseError(FrameHeaderParseError),

    /// Failed to handle a CQL event (server response received on stream -1).
    #[error("Failed to handle server event: {0}")]
    CqlEventHandlingError(#[from] CqlEventHandlingError),

    /// Received a server frame with unexpected stream id.
    #[error("Received a server frame with unexpected stream id: {0}")]
    UnexpectedStreamId(i16),

    /// IO error - server failed to write data to the socket.
    #[error("Failed to write data: {0}")]
    WriteError(std::io::Error),

    /// Maximum number of orphaned streams exceeded.
    #[error("Too many orphaned stream ids: {0}")]
    TooManyOrphanedStreamIds(u16),

    /// Failed to send data via tokio channel. This implies
    /// that connection was probably already broken for some other reason.
    #[error(
        "Failed to send/receive data needed to perform a request via tokio channel.
        It implies that other half of the channel has been dropped.
        The connection was already broken for some other reason."
    )]
    ChannelError,
}

impl From<BrokenConnectionErrorKind> for BrokenConnectionError {
    fn from(value: BrokenConnectionErrorKind) -> Self {
        BrokenConnectionError(Arc::new(value))
    }
}

/// Failed to handle a CQL event received on a stream -1.
/// Possible error kinds are:
/// - failed to deserialize response's frame header
/// - failed to deserialize CQL event response
/// - received invalid server response
/// - failed to send an event info via channel (connection is probably broken)
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CqlEventHandlingError {
    /// Received an EVENT server response, but failed to deserialize it.
    #[error("Failed to deserialize EVENT response: {0}")]
    CqlEventParseError(#[from] CqlEventParseError),

    /// Received an unexpected response on stream -1.
    #[error("Received unexpected server response on stream -1: {0}. Expected EVENT response")]
    UnexpectedResponse(CqlResponseKind),

    /// Failed to deserialize body extensions of frame received on stream -1.
    #[error("Failed to deserialize a header of frame received on stream -1: {0}")]
    BodyExtensionParseError(#[from] FrameBodyExtensionsParseError),

    /// Driver failed to send event data between the internal tasks.
    /// It implies that connection was broken for some reason.
    #[error("Failed to send event info via channel. The channel is probably closed, which is caused by connection being broken")]
    SendError,
}

/// An error that occurred during execution of
/// - `QUERY`
/// - `PREPARE`
/// - `EXECUTE`
/// - `BATCH`
///
/// request. This error represents a definite request failure, unlike
/// [`RequestAttemptError`] which represents a failure of a single
/// attempt.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum RequestError {
    /// Load balancing policy returned an empty plan.
    #[error(
            "Load balancing policy returned an empty plan.\
            First thing to investigate should be the logic of custom LBP implementation.\
            If you think that your LBP implementation is correct, or you make use of `DefaultPolicy`,\
            then this is most probably a driver bug!"
        )]
    EmptyPlan,

    /// Selected node's connection pool is in invalid state.
    #[error("No connections in the pool: {0}")]
    ConnectionPoolError(#[from] ConnectionPoolError),

    /// Failed to run a request within a provided client timeout.
    #[error(
            "Request execution exceeded a client timeout of {}ms",
            std::time::Duration::as_millis(.0)
        )]
    RequestTimeout(std::time::Duration),

    /// Failed to execute request.
    #[error(transparent)]
    LastAttemptError(#[from] RequestAttemptError),
}

impl RequestError {
    /// Converts (widens) this error into an [`ExecutionError`].
    pub fn into_execution_error(self) -> ExecutionError {
        match self {
            RequestError::EmptyPlan => ExecutionError::EmptyPlan,
            RequestError::ConnectionPoolError(e) => e.into(),
            RequestError::RequestTimeout(dur) => ExecutionError::RequestTimeout(dur),
            RequestError::LastAttemptError(e) => ExecutionError::LastAttemptError(e),
        }
    }
}

/// An error that occurred during a single attempt of:
/// - `QUERY`
/// - `PREPARE`
/// - `EXECUTE`
/// - `BATCH`
///
/// requests. The retry decision is made based
/// on this error.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum RequestAttemptError {
    /// Failed to serialize query parameters. This error occurs, when user executes
    /// a CQL `QUERY` request with non-empty parameter's value list and the serialization
    /// of provided values fails during statement preparation.
    #[error("Failed to serialize query parameters: {0}")]
    SerializationError(#[from] SerializationError),

    /// Failed to serialize CQL request.
    #[error("Failed to serialize CQL request: {0}")]
    CqlRequestSerialization(#[from] CqlRequestSerializationError),

    /// Driver was unable to allocate a stream id to execute a query on.
    #[error("Unable to allocate stream id")]
    UnableToAllocStreamId,

    /// A connection has been broken during query execution.
    #[error(transparent)]
    BrokenConnectionError(#[from] BrokenConnectionError),

    /// Failed to deserialize frame body extensions.
    #[error(transparent)]
    BodyExtensionsParseError(#[from] FrameBodyExtensionsParseError),

    /// Received a RESULT server response, but failed to deserialize it.
    #[error(transparent)]
    CqlResultParseError(#[from] CqlResultParseError),

    /// Received an ERROR server response, but failed to deserialize it.
    #[error("Failed to deserialize ERROR response: {0}")]
    CqlErrorParseError(#[from] CqlErrorParseError),

    /// Database sent a response containing some error with a message
    #[error("Database returned an error: {0}, Error message: {1}")]
    DbError(DbError, String),

    /// Received an unexpected response from the server.
    #[error(
        "Received unexpected response from the server: {0}. Expected RESULT or ERROR response."
    )]
    UnexpectedResponse(CqlResponseKind),

    /// Prepared statement id changed after repreparation.
    #[error(
        "Prepared statement id changed after repreparation; md5 sum (computed from the query string) should stay the same;\
        Statement: \"{statement}\"; expected id: {expected_id:?}; reprepared id: {reprepared_id:?}"
    )]
    RepreparedIdChanged {
        /// The CQL statement that was reprepared.
        statement: String,
        /// Expected id of the prepared statement.
        /// This is the id that was returned by the server
        /// when the statement was prepared for the first time.
        expected_id: Vec<u8>,
        /// The id of the prepared statement returned by the server
        /// when the statement was reprepared.
        reprepared_id: Vec<u8>,
    },

    /// Driver tried to reprepare a statement in the batch, but the reprepared
    /// statement's id is not included in the batch.
    #[error("Reprepared statement's id does not exist in the batch.")]
    RepreparedIdMissingInBatch,

    /// A result with nonfinished paging state received for unpaged query.
    #[error("Unpaged query returned a non-empty paging state! This is a driver-side or server-side bug.")]
    NonfinishedPagingState,
}

impl From<response::error::Error> for RequestAttemptError {
    fn from(value: response::error::Error) -> Self {
        RequestAttemptError::DbError(value.error, value.reason)
    }
}

impl From<InternalRequestError> for RequestAttemptError {
    fn from(value: InternalRequestError) -> Self {
        match value {
            InternalRequestError::CqlRequestSerialization(e) => e.into(),
            InternalRequestError::BodyExtensionsParseError(e) => e.into(),
            InternalRequestError::CqlResponseParseError(e) => match e {
                // Only possible responses are RESULT and ERROR. If we failed parsing
                // other response, treat it as unexpected response.
                CqlResponseParseError::CqlErrorParseError(e) => e.into(),
                CqlResponseParseError::CqlResultParseError(e) => e.into(),
                _ => RequestAttemptError::UnexpectedResponse(e.to_response_kind()),
            },
            InternalRequestError::BrokenConnection(e) => e.into(),
            InternalRequestError::UnableToAllocStreamId => {
                RequestAttemptError::UnableToAllocStreamId
            }
        }
    }
}

/// An error that occurred when performing a request.
///
/// Possible error kinds:
/// - Connection is broken
/// - Response's frame header deserialization error
/// - CQL response (frame body) deserialization error
/// - Driver was unable to allocate a stream id for a request
///
/// This is driver's internal low-level error type. It can occur
/// during any request execution in connection layer.
#[derive(Error, Debug)]
#[non_exhaustive]
pub(crate) enum InternalRequestError {
    /// Failed to serialize CQL request.
    #[error("Failed to serialize CQL request: {0}")]
    CqlRequestSerialization(#[from] CqlRequestSerializationError),

    /// Failed to deserialize frame body extensions.
    #[error(transparent)]
    BodyExtensionsParseError(#[from] FrameBodyExtensionsParseError),

    /// Failed to deserialize a CQL response (frame body).
    #[error(transparent)]
    CqlResponseParseError(#[from] CqlResponseParseError),

    /// A connection was broken during request execution.
    #[error(transparent)]
    BrokenConnection(#[from] BrokenConnectionError),

    /// Driver was unable to allocate a stream id to execute a request on.
    #[error("Unable to allocate a stream id")]
    UnableToAllocStreamId,
}

impl From<ResponseParseError> for InternalRequestError {
    fn from(value: ResponseParseError) -> Self {
        match value {
            ResponseParseError::BodyExtensionsParseError(e) => e.into(),
            ResponseParseError::CqlResponseParseError(e) => e.into(),
        }
    }
}

/// An error type returned from `Connection::parse_response`.
/// This is driver's internal type.
#[derive(Error, Debug)]
pub(crate) enum ResponseParseError {
    #[error(transparent)]
    BodyExtensionsParseError(#[from] FrameBodyExtensionsParseError),
    #[error(transparent)]
    CqlResponseParseError(#[from] CqlResponseParseError),
}

/// Error returned from [ClusterState](crate::cluster::ClusterState) APIs.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ClusterStateTokenError {
    /// Failed to calculate token.
    #[error(transparent)]
    TokenCalculation(#[from] TokenCalculationError),

    /// Failed to serialize values required to compute partition key.
    #[error(transparent)]
    Serialization(#[from] SerializationError),

    /// `ClusterState` doesn't currently have metadata for the requested table.
    #[error("Can't find metadata for requested table ({keyspace}.{table}).")]
    UnknownTable {
        /// Keyspace name for which the error occurred.
        keyspace: String,
        /// Table name for which the error occurred.
        table: String,
    },
}

#[cfg(test)]
mod tests {
    use scylla_cql::Consistency;

    use super::{DbError, ExecutionError, RequestAttemptError, WriteType};

    #[test]
    fn write_type_from_str() {
        let test_cases: [(&str, WriteType); 9] = [
            ("SIMPLE", WriteType::Simple),
            ("BATCH", WriteType::Batch),
            ("UNLOGGED_BATCH", WriteType::UnloggedBatch),
            ("COUNTER", WriteType::Counter),
            ("BATCH_LOG", WriteType::BatchLog),
            ("CAS", WriteType::Cas),
            ("VIEW", WriteType::View),
            ("CDC", WriteType::Cdc),
            ("SOMEOTHER", WriteType::Other("SOMEOTHER".to_string())),
        ];

        for (write_type_str, expected_write_type) in &test_cases {
            let write_type = WriteType::from(*write_type_str);
            assert_eq!(write_type, *expected_write_type);
        }
    }

    // A test to check that displaying DbError and ExecutionError::DbError works as expected
    // - displays error description
    // - displays error parameters
    // - displays error message
    // - indented multiline strings don't cause whitespace gaps
    #[test]
    fn dberror_full_info() {
        // Test that DbError::Unavailable is displayed correctly
        let db_error = DbError::Unavailable {
            consistency: Consistency::Three,
            required: 3,
            alive: 2,
        };

        let db_error_displayed: String = format!("{db_error}");

        let mut expected_dberr_msg =
            "Not enough nodes are alive to satisfy required consistency level ".to_string();
        expected_dberr_msg += "(consistency: Three, required: 3, alive: 2)";

        assert_eq!(db_error_displayed, expected_dberr_msg);

        // Test that ExecutionError::DbError::(DbError::Unavailable) is displayed correctly
        let execution_error = ExecutionError::LastAttemptError(RequestAttemptError::DbError(
            db_error,
            "a message about unavailable error".to_string(),
        ));
        let execution_error_displayed: String = format!("{execution_error}");

        let mut expected_execution_err_msg = "Database returned an error: ".to_string();
        expected_execution_err_msg += &expected_dberr_msg;
        expected_execution_err_msg += ", Error message: a message about unavailable error";

        assert_eq!(execution_error_displayed, expected_execution_err_msg);
    }
}
