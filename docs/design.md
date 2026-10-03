# Design

Status: PR 1 stub. Revised at the end of each phase.

## Purpose

Ingest ASTERIX CAT048 monoradar target reports over UDP, decode them, and process them under a latency bound that is measured, not claimed. The deliverable is the feed handler and its proven performance envelope.

## Components

| Crate | Kind | Role |
| --- | --- | --- |
| `asterix` | library | Sans-IO CAT048 decode and encode. No sockets, clocks or async runtime. |
| `feed-handler` | binary | Receives datagrams, decodes, queues, processes, counts. |
| `loadgen` | binary | Sends CAT048 on a fixed open-loop schedule, with sequence number and timestamp in the SP field. |
| `harness` | binary | Sweeps send rates, collects reports, produces the throughput / latency / drop table. |

## Data flow

```
loadgen ──UDP──▶ [kernel socket buffer] ──▶ receive ──▶ [bounded queue] ──▶ process ──▶ done
   │                                                                          │
   └── seq + intended send time (SP field)          latency recorded here ◀───┘
```

## Latency definition

- **Start:** the time the generator *intended* to send the message, per its fixed schedule, read from a system-wide monotonic clock and carried in the SP field. Not the time it actually sent, which would hide generator lag (coordinated omission).
- **End:** the time the processing stage finishes with the message, read from the same monotonic clock.
- **Reported as:** p50 / p99 / p999 / max. Averages are not reported as a headline number.
- Per-stage timestamps (received, dequeued, done) split latency into network-plus-kernel, queueing and processing time.

## Where messages can be lost

1. Kernel socket receive buffer overflow (receiver too slow, burst too large).
2. Application bounded queue full (counted, drop-newest via `try_send`).
3. Decode failure (malformed or truncated input; counted, never a panic).

Every run must satisfy: *sent = processed + app-dropped + kernel-dropped + decode-failed + unexplained*, with unexplained = 0.

## Non-goals

- No tracker: no association, smoothing or prediction.
- No display.
- No distributed deployment; single machine, loopback.
- Linux only for measurement: kernel drop counters and socket behavior are Linux-specific.

## Open questions

- Exact SP field layout (width and endianness of sequence number and timestamp).
- How to read the generator's monotonic time in the handler process without clock-source mismatch.
- Which CAT048 items make up the fully decoded core subset beyond those listed in the plan.