# [nostr-rs-bench](https://git.sr.ht/~gheartsfield/nostr-rs-bench)

This is a [nostr](https://github.com/nostr-protocol/nostr) relay
benchmark suite, written in Rust.  It currently is designed to help
benchmark the `nostr-rs-relay` implementation, but could be used for
others.

Each benchmark is documented below.

## Random Event EOSE Concurrency

Benchmark `random_event_eose_concurrent` is designed to test REQ
concurrency against a very simple query that returns no results.
Since no results are returned, we are not benchmarking network
bandwidth, just the ability to successfully run and return an `EOSE`
response.

A number of clients are created that each issue a REQ for a single
random event ID.  Once an `EOSE` response is received, it is
timestamped as a successful result, and recorded.  The REQ is then
replaced with another REQ for a different random event ID.  A `NOTICE`
is treated as an error.

The test is parameterized by the number of concurrent clients, and how
many requests are sent.

The test results contain the client number, result type, and duration
in milliseconds, example output for
`random_event_eose_concurrent(clients=3,requests=5)`:

```
client id, result type, duration_ms
2,EOSE,1.935
1,EOSE,4.837
1,EOSE,32.394
2,EOSE,13.264
3,NOTICE,342.823
```

A summary statistic of the entire test duration in seconds, and the
number of success/failures is also provided:

```
test_duration_s, success_count, failure_count
0.345,4,1
```
