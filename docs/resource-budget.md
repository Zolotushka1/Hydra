# Resource budget

Hydra targets a deployment baseline of **1 vCPU, 512 MB RAM, 10 GB disk**. This
is a design constraint rather than an aspiration: the panel and the agent are
expected to run together with Xray on the cheapest tier of a commodity VPS, and
features that only work on larger hardware are out of scope.

## What follows from it

**Nothing unbounded lives in memory.** There are no unbounded queues, caches or
histories. Every event and history buffer has a configured cap, and compaction
keeps active and recent operational state while dropping terminal and old records
first.

**Collection sizes are observable.** `GET /api/system/resource-budget` reports
tracked runtime collection sizes against their configured limits, so low-memory
behaviour can be validated rather than assumed. The `panel_memory_budget` system
alert fires when panel process RSS crosses the configured warning and critical
thresholds relative to the 512 MB budget.

**Pagination is `limit`-only.** There is no `offset` and no cursor. At 512 MB
deep paging is unreachable, so an `offset` parameter would advertise something
the panel cannot honour. One helper, `resolve_limit`, applies the rule
everywhere: an omitted `limit` means the configured maximum, a larger value
clamps down to it, and `0` becomes `1`. A `limit` can narrow a result, never
widen it. Narrowing beyond that is the job of the filters each endpoint already
has.

Maximums come from `runtime_limits` — a collection's own buffer cap where one
exists, otherwise `max_list_page_size`, which defaults to 200.

Internal full enumeration is a separate path. Config generation must see every
user and every client, so it goes through dedicated `all()` and `all_clients()`
methods rather than the paged list API. Those are deliberately unbounded and are
never used to serve an API read.

**Durability is spent where loss is unrecoverable.** Files whose loss cannot be
recovered, and files carrying secret material, are replaced with a full fsync
sequence. Bounded telemetry buffers are not: they drop old records anyway, and an
fsync per event costs more at 1 vCPU than the last few records are worth. See
[Persistence](persistence.md) for the split.

**Every persisted secret has a stated reason and lifecycle**, and background work
is visible to the operator rather than implicit. Explicit control-plane state is
preferred over hidden runtime state, because hidden state cannot be budgeted.

The load and memory acceptance procedure is documented in
[`panel/docs/load-memory-validation.md`](../panel/docs/load-memory-validation.md).

## Agent side

The same baseline applies to the node agent, where it means: keep the agent small
and predictable, buffer no unbounded logs or metrics in memory, avoid complex
background job graphs, and keep the authentication, config fetch, sync report and
heartbeat flows explicit.
