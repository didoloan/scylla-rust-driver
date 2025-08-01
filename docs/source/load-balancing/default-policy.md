# DefaultPolicy

`DefaultPolicy` is the default load balancing policy in ScyllaDB Rust Driver. It
can be configured to be datacenter-aware and token-aware. Datacenter failover
for queries with non-local consistency mode is also supported.

## Creating a DefaultPolicy

`DefaultPolicy` can be created only using `DefaultPolicyBuilder`. The
`builder()` method of `DefaultPolicy` returns a new instance of
`DefaultPolicyBuilder` with the following default values:

- `preferences`: no particular datacenter/rack preference
- `is_token_aware`: `true`
- `permit_dc_failover`: `false`
- `latency_awareness`: `None`

You can use the builder methods to configure the desired settings and create a
`DefaultPolicy` instance:

```rust
# extern crate scylla;
# fn test_if_compiles() {
use scylla::policies::load_balancing::DefaultPolicy;

let default_policy = DefaultPolicy::builder()
        .prefer_datacenter_and_rack("dc1".to_string(), "rack1".to_string())
        .token_aware(true)
        .permit_dc_failover(true)
        .build();
# }
```

### Semantics of `DefaultPolicy`

#### Preferences

The `preferences` field in `DefaultPolicy` allows the load balancing
policy to prioritize nodes based on their location. It has three modes:

- no preference
- preferred datacenter
- preferred datacenter and rack

When a datacenter `"my_dc"` is preferred, the policy will treat nodes in `"my_dc"`
as "local" nodes, and nodes in other datacenters as "remote" nodes. This affects
the order in which nodes are returned by the policy when selecting nodes for
read or write operations. If no datacenter is preferred, the policy will treat
all nodes as local nodes.

`preferences` allow the load balancing policy to prioritize nodes based on their
availability zones (racks) in the preferred datacenter, too. When a datacenter
and a rack are preferred, the policy will first return replicas in the local rack
in the preferred datacenter, and then the other replicas in the datacenter
(followed by remote replicas). After replicas, the other node will be ordered
similarly, too (local rack nodes, local datacenter nodes, remote nodes).

When datacenter failover is disabled (`permit_dc_failover` is set to
false), the default policy will only include local nodes in load balancing
plans. Remote nodes will be excluded, even if they are alive and available to
serve requests.

#### Datacenter Failover

In the event of a datacenter outage or network failure, the nodes in that
datacenter may become unavailable, and clients may no longer be able to access
the data stored on those nodes. To address this, the `DefaultPolicy` supports
datacenter failover, which allows to route requests to nodes in other datacenters
if the local nodes are unavailable.

Datacenter failover can be enabled in `DefaultPolicy` by `permit_dc_failover`
setting in the builder. When this flag is set, the policy will prefer to return
alive remote replicas if datacenter failover is permitted and possible due to
consistency constraints.

#### Token awareness

Token awareness refers to a mechanism by which the driver is aware of the token
range assigned to each node in the cluster. Tokens are assigned to nodes to
partition the data and distribute it across the cluster.

When a user wants to read or write data, the driver can use token awareness to
route the request to the correct node based on the token range of the data
being accessed. This can help to minimize network traffic and improve
performance by ensuring that the data is accessed locally as much as possible.

In the case of `DefaultPolicy`, token awareness is enabled by default, meaning
that the policy will prefer to return alive local replicas if the token is
available. This means that if the client is requesting data that falls within
the token range of a particular node, the policy will try to route the request
to that node first, assuming it is alive and responsive.

Token awareness can significantly improve the performance and scalability of
applications built on Scylla. By using token awareness, users can ensure that
data is accessed locally as much as possible, reducing network overhead and
improving throughput.

Please note that for token awareness to be applied, a statement must be
prepared before being executed.

### Latency awareness

Latency awareness is a mechanism that penalises nodes whose measured recent
average latency classifies it as falling behind the others.

Every `update_rate` the global minimum average latency is computed,
and all nodes whose average latency is worse than `exclusion_threshold`
times the global minimum average latency become penalised for
`retry_period`. Penalisation involves putting those nodes at the very end
of the query plan. As it is often not truly beneficial to prefer
faster non-replica than replicas lagging behind the non-replicas,
this mechanism may as well worsen latencies and/or throughput.

> **Warning**
>
> Using latency awareness is **NOT** recommended, unless prior
>benchmarks prove its beneficial impact on the specific workload's
>performance. Use with caution.

### Creating a latency aware DefaultPolicy

```rust
# extern crate scylla;
# fn example() {
use scylla::policies::load_balancing::{
    LatencyAwarenessBuilder, DefaultPolicy
};
use std::time::Duration;

let latency_awareness_builder = LatencyAwarenessBuilder::new()
    .exclusion_threshold(3.)
    .update_rate(Duration::from_secs(3))
    .retry_period(Duration::from_secs(30))
    .minimum_measurements(200);

let policy = DefaultPolicy::builder()
        // Here further customisation is, of course, possible.
        // e.g.: .prefer_datacenter(...)
        .latency_awareness(latency_awareness_builder)
        .build();
# }
```

```rust
# extern crate scylla;
# fn test_if_compiles() {
use scylla::policies::load_balancing::DefaultPolicy;

let default_policy = DefaultPolicy::builder()
        .prefer_datacenter("dc1".to_string())
        .token_aware(true)
        .permit_dc_failover(true)
        .build();
# }
```

### Node order in produced plans

The DefaultPolicy prefers to return nodes in the following order:

1. Alive local replicas (if token is available & token awareness is enabled)
2. Alive remote replicas (if datacenter failover is permitted & possible due to consistency constraints)
3. Alive local nodes
4. Alive remote nodes (if datacenter failover is permitted & possible due to consistency constraints)
5. Enabled down nodes
And only if latency awareness is enabled:
6. Penalised: alive local replicas, alive remote replicas, ... (in order as above).

If no preferred datacenter is specified, all nodes are treated as local ones.

Replicas in the same priority groups are shuffled[^1]. Non-replicas are randomly
rotated (similarly to a round robin with a random index).

[^1]: There is an optimisation implemented for LWT requests that routes them
to the replicas in the ring order (as it prevents contention due to Paxos conflicts), so replicas in that case are not shuffled in groups at all.
In order for the optimisation to be applied, LWT statements must be prepared before.
