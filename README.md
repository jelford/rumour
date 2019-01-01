# rumour

Project status: experimental. 

Implemented:
- Node failure detection
- Peer exchange
- Commandline that prints out node join and failure events

## goal

The primary goal is for me to learn a bit of Rust, and familiarize myself with Tokio, futures,
and the other important parts of Rust's networking story.

An implementation of the [SWIM Protocol](https://www.cs.cornell.edu/~asdas/research/dsn02-swim.pdf) in safe, asyncronous rust.

The implementation is based on HashiCorp's [Serf](https://www.serf.io/docs/internals/gossip.html).

Constraints:
- should be easily embeddable as a library in C applications
- should not require additional runtime - does not start threads or require a garbage collector
- should use 100% safe Rust code
- events issued through callbacks to application code

Stretch goals:
- custom event propogation beyond cluster membership
- bring-your-own async event loop, so that the library doesn't need to take ownership of a thread
- Serf-style commnandline application to allow users to interact with the cluster without using the library

## development instructions

### Run

    cargo run -- --port 2073 --peeraddress 127.0.0.1:2074 &
    cargo run -- --port 2074 --peeraddress 127.0.0.1:2073 &

Runs two instances of the commandline utility that will form a cluster. 
Each node will print messages when the other node joins or leaves the
cluster. 

### Code structure

* `src/node/mod.rs` contains code for initializing and running a `rumour` server
It knows about creating a Tokio reactor and wiring together components.
* `src/node/mill.rs` contains the core logic. Tests in here stub out the Tokio
reactor, but should otherwise be a good high-level description of `rumour`'s behaviour.

Tests in other modules are at unit-level, rather than module-level.

