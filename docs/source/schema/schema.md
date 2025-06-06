# Schema

The driver is capable of fetching database schema and presenting it to its users.

## Fetching schema

Fetching database schema occurs periodically, but it can also be done on-demand. In order to fetch the newest database schema, one can call `refresh_metadata()` on a Session instance: 
```rust
# extern crate scylla;
# extern crate tokio;
# use std::error::Error;
# use scylla::client::session::Session;
# use scylla::client::session_builder::SessionBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let uri = std::env::var("SCYLLA_URI")
        .unwrap_or_else(|_| "127.0.0.1:9042".to_string());

    let session: Session = SessionBuilder::new().known_node(uri).build().await?;
    // Schema metadata will be fetched below
    session.refresh_metadata().await?;
    Ok(())
}
```

## Inspecting schema

Once fetched, a snapshot of cluster's schema can be examined. The following information can be obtained:
 - keyspace
   - tables belonging to the keyspace
   - materialized views belonging to the keyspace
   - replication strategy
   - user-defined types
 - table/view
   - primary key definition
   - columns
   - partitioner type

Example showing how to print obtained schema information:

```rust
# extern crate scylla;
# extern crate tokio;
# use std::error::Error;
# use scylla::client::session::Session;
# use scylla::client::session_builder::SessionBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let uri = std::env::var("SCYLLA_URI")
        .unwrap_or_else(|_| "127.0.0.1:9042".to_string());

    let session: Session = SessionBuilder::new().known_node(uri).build().await?;
    // Schema metadata will be fetched below
    session.refresh_metadata().await?;

    let cluster_state = &session.get_cluster_state();
    let keyspaces_iter = cluster_state.keyspaces_iter();

    for (keyspace_name, keyspace_info) in keyspaces_iter {
        println!("Keyspace {}:", keyspace_name);
        println!("\tTables: {:#?}", keyspace_info.tables);
        println!("\tViews: {:#?}", keyspace_info.views);
        println!("\tUDTs: {:#?}", keyspace_info.user_defined_types);
    }

    Ok(())
}
```
