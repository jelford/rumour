# rumour

Project status: experimental.

## goal

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


