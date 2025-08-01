# Downgrading consistency retry policy

A retry policy that sometimes retries with a lower consistency level than the one initially
requested.
**BEWARE**: this policy may retry queries using a lower consistency level than the one
initially requested. By doing so, it may break consistency guarantees. In other words, if you use
this retry policy, there are cases (documented below) where a read at `Consistency::Quorum` **may
not** see a preceding write at `Consistency::Quorum`. Do not use this policy unless you have
understood the cases where this can happen and are ok with that. It is also highly recommended to
always log the occurrences of such consistency breaks.
This policy implements the same retries than the [DefaultRetryPolicy](default.md) policy. But on top
of that, it also retries in the following cases:
  - On a read timeout: if the number of replicas that responded is greater than one, but lower
    than is required by the requested consistency level, the operation is retried at a lower
    consistency level.
  - On a write timeout: if the operation is a `WriteType::UnloggedBatch` and at least one
    replica acknowledged the write, the operation is retried at a lower consistency level.
    Furthermore, for other operations, if at least one replica acknowledged the write, the
    timeout is ignored.
  - On an unavailable exception: if at least one replica is alive, the operation is retried at
    a lower consistency level.

The lower consistency level to use for retries is determined by the following rules:
  - if more than 3 replicas responded, use `Consistency::Three`.
  - if 1, 2 or 3 replicas responded, use the corresponding level `Consistency::One`, `Consistency::Two` or
      `Consistency::Three`.

Note that if the initial consistency level was `Consistency::EachQuorum`, ScyllaDB returns the number
of live replicas _in the datacenter that failed to reach consistency_, not the overall
number in the cluster. Therefore if this number is 0, we still retry at `Consistency::One`, on the
assumption that a host may still be up in another datacenter.
The reasoning being this retry policy is the following one. If, based on the information the
ScyllaDB coordinator node returns, retrying the operation with the initially requested
consistency has a chance to succeed, do it. Otherwise, if based on this information we know
**the initially requested consistency level cannot be achieved currently**, then:
  - For writes, ignore the exception (thus silently failing the consistency requirement) if we
    know the write has been persisted on at least one replica.
  - For reads, try reading at a lower consistency level (thus silently failing the consistency
    requirement).
In other words, this policy implements the idea that if the requested consistency level cannot be
achieved, the next best thing for writes is to make sure the data is persisted, and that reading
something is better than reading nothing, even if there is a risk of reading stale data.

This policy is based on the one in [DataStax Java Driver](https://docs.datastax.com/en/drivers/java/3.11/com/datastax/driver/core/policies/DowngradingConsistencyRetryPolicy.html).
The behaviour is the same.

### Examples
To use in `Session`:
```rust
# extern crate scylla;
# use scylla::client::session::Session;
# use std::error::Error;
# use std::sync::Arc;
# async fn check_only_compiles() -> Result<(), Box<dyn Error>> {
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::client::execution_profile::ExecutionProfile;
use scylla::policies::retry::DowngradingConsistencyRetryPolicy;

let handle = ExecutionProfile::builder()
    .retry_policy(Arc::new(DowngradingConsistencyRetryPolicy::new()))
    .build()
    .into_handle();

let session: Session = SessionBuilder::new()
    .known_node("127.0.0.1:9042")
    .default_execution_profile_handle(handle)
    .build()
    .await?;
# Ok(())
# }
```

To use in an [unprepared statement](../statements/unprepared.md):
```rust
# extern crate scylla;
# use scylla::client::session::Session;
# use std::error::Error;
# use std::sync::Arc;
# async fn check_only_compiles(session: &Session) -> Result<(), Box<dyn Error>> {
use scylla::statement::unprepared::Statement;
use scylla::client::execution_profile::ExecutionProfile;
use scylla::policies::retry::DowngradingConsistencyRetryPolicy;

let handle = ExecutionProfile::builder()
    .retry_policy(Arc::new(DowngradingConsistencyRetryPolicy::new()))
    .build()
    .into_handle();

// Create a Statement manually and set the retry policy
let mut my_statement: Statement = Statement::new("INSERT INTO ks.tab (a) VALUES(?)");
my_statement.set_execution_profile_handle(Some(handle));

// Execute the statement using this retry policy
let to_insert: i32 = 12345;
session.query_unpaged(my_statement, (to_insert,)).await?;
# Ok(())
# }
```

To use in a [prepared statement](../statements/prepared.md):
```rust
# extern crate scylla;
# use scylla::client::session::Session;
# use std::error::Error;
# use std::sync::Arc;
# async fn check_only_compiles(session: &Session) -> Result<(), Box<dyn Error>> {
use scylla::statement::prepared::PreparedStatement;
use scylla::client::execution_profile::ExecutionProfile;
use scylla::policies::retry::DowngradingConsistencyRetryPolicy;

let handle = ExecutionProfile::builder()
    .retry_policy(Arc::new(DowngradingConsistencyRetryPolicy::new()))
    .build()
    .into_handle();

// Create PreparedStatement manually and set the retry policy
let mut prepared: PreparedStatement = session
    .prepare("INSERT INTO ks.tab (a) VALUES(?)")
    .await?;

prepared.set_execution_profile_handle(Some(handle));


// Execute the statement using this retry policy
let to_insert: i32 = 12345;
session.execute_unpaged(&prepared, (to_insert,)).await?;
# Ok(())
# }
```
