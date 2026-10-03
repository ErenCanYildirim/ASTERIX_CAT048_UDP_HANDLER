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

## Framing (PR 2)

A datagram is split into data blocks (`CAT | LEN | records`). Record splitting needs FSPEC parsing and happens in the record layer.

| Input | Decision |
| --- | --- |
| Empty datagram | Error. A datagram with no blocks carries nothing and should be counted as a decode failure. |
| Fewer than 3 bytes left for a header | Error, including trailing bytes after valid blocks. |
| LEN < 3 | Error: impossible, LEN includes the header. |
| LEN = 3 | Error: the spec requires at least one record per block. |
| LEN > bytes remaining | Error. |
| Non-048 category | Not an error. Framing is category-agnostic; the caller decides. |

- **After an error, iteration stops.** A wrong LEN means the next block's start is unknown, so continuing would parse garbage as headers. Blocks before the error are still returned.
- **Iterator of `Result`, not `Result<Vec<_>>`:** no allocation, and good blocks before a bad one are not thrown away.
- **Zero-copy:** blocks borrow their record bytes from the datagram buffer.

## Records and items (PR 3)

A CAT048 block's record bytes are split into records, and each record into raw item slices, by walking the FSPEC and the UAP. No item is interpreted yet.

- **UAP is a table, not code.** Each FRN maps to an item and one of five encodings (fixed, extended, repetitive, explicit, compound). One generic length function handles all of them.
- **UAP edition:** CAT048 v1.32, cross-checked item by item against the independent machine-readable definitions in `zoranbosnjak/asterix-specs`. I048/030 is "repetitive FX" there; on the wire it has the same length rule as an extended item.
- **Every item's length is implemented**, even items we never interpret, because a record has no length field: skipping an item requires knowing its length.

| Input | Decision |
| --- | --- |
| Non-CAT048 block | Error up front (`WrongCategory`). |
| FSPEC with FX set and no next byte | Error. |
| FSPEC with FX set in octet 4 | Error: there is no FRN 29. |
| FSPEC flagging no items | Error: an empty record carries nothing. |
| Item longer than the remaining block | Error, naming the item and its offset. |
| Repetitive item with REP = 0 | Allowed: unambiguous length, harmless. |
| Explicit item with LEN = 0 | Error: LEN counts itself, so 0 is impossible. |
| Compound item flagging an undefined subfield | Error: its length is unknown. |

- **After an error, iteration stops**, for the same reason as framing: the next record's start is unknown.
- **Items are stored in a fixed array indexed by FRN** (`[Option<&[u8]>; 28]`): O(1) lookup, no allocation.

## Non-goals

- No tracker: no association, smoothing or prediction.
- No display.
- No distributed deployment; single machine, loopback.
- Linux only for measurement: kernel drop counters and socket behavior are Linux-specific.

## Open questions

- Exact SP field layout (width and endianness of sequence number and timestamp).
- How to read the generator's monotonic time in the handler process without clock-source mismatch.
- Which CAT048 items make up the fully decoded core subset beyond those listed in the plan.