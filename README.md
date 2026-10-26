# respcache

An in-memory key-value cache server in Rust (standard library only) that speaks a subset of the Redis
protocol (RESP), so `redis-cli` and Redis client libraries can talk to it.

- **Protocol**: an incremental RESP parser (`src/resp.rs`) that handles arrays of bulk strings and inline
  commands, and returns "incomplete" when a command is split across TCP reads, so the server keeps reading
  until it has a whole command.
- **Cache**: LRU eviction at a configurable key limit plus optional per-key expiry (`src/cache.rs`). Recency
  is a counter mapped to keys in a `BTreeMap`, so get, set and eviction are O(log n). Expired keys are removed
  when touched.
- **Server**: one thread per connection, sharing the cache behind a mutex (`src/lib.rs`).

Commands: `PING`, `ECHO`, `GET`, `SET key value [EX seconds | PX milliseconds]`, `DEL`, `EXISTS`, `INCR`,
`EXPIRE`, `TTL`, `DBSIZE`.

## Run

```sh
cargo run --release -- 127.0.0.1:6380 10000     # address, max keys
redis-cli -p 6380 set greeting hello
redis-cli -p 6380 get greeting
```

## Tests

```sh
cargo test
```

Unit tests cover the parser (every split point of a command returns "incomplete", back-to-back commands,
malformed input) and the cache (LRU order, overwrite without eviction, TTL and expiry). Integration tests
start the server on a random port and check SET/GET/DEL/INCR/TTL, a command split across two writes, and
eviction over TCP. CI runs clippy (warnings as errors) and the tests on every push.

Not implemented: persistence, replication, pub/sub, and most Redis commands.
