# PR 3.2 media port

## Module map

| Rust module | TypeScript source | Responsibility |
|---|---|---|
| `media::policy` | `buffering-policy.ts` | Pure measured parallelism and buffer targets |
| `media::range` | `media-range.ts` | Exact intervals, sizes, encodings and validators |
| `media::retry` | `media-retry.ts` | Delta-seconds and HTTP-date Retry-After |
| `media::cache::{mod,disk}` | `media-cache.ts` | Reservations, pins, LRU, disk admission and owner cleanup |
| `media::fetch` | `media-fetch.ts` | Injectable streaming transport; rustls and manual redirects |
| `media::proxy::{mod,server}` | `media-proxy.ts`, `loopback-port.ts` | Sessions, ephemeral IPv4 listener, tracked connections and bounded shutdown |
| `media::proxy::{session,probe,scheduler,download,events}` | `media-proxy.ts` | Streaming chunks, warm-up, retries, refresh, measurements and sanitized events |

## Test mapping

Rust names below are under `media::<module>::tests`. All eight incompatible-header
fixtures remain assertions in the same parametrized-equivalent Rust test. No
policy/proxy assertion exists in `playback-buffering.test.ts`: its sole test loads
browser HLS configuration.

| TS file / test title | Rust test |
|---|---|
| `media-range`: accepts the exact interval including a short final chunk | `range::accepts_the_exact_interval_including_a_short_final_chunk` |
| `media-range`: rejects incompatible headers %j | `range::rejects_incompatible_headers_j` (all eight fixtures) |
| `media-range`: prefers a strong ETag and excludes weak tags | `range::prefers_a_strong_etag_and_excludes_weak_tags` |
| `media-range`: preserves long server deadlines and rejects malformed values | `range::preserves_long_server_deadlines_and_rejects_malformed_values` |
| `buffering-policy`: does not probe capacity during paused or intentionally idle downloads | `policy::does_not_probe_capacity_during_paused_or_intentionally_idle_downloads` |
| `buffering-policy`: reverts an extra connection when useful throughput does not improve | `policy::reverts_an_extra_connection_when_useful_throughput_does_not_improve` |
| `buffering-policy`: retains a useful increase, then respects throttling and buffer limits | `policy::retains_a_useful_increase_then_respects_throttling_and_buffer_limits` |
| `media-cache`: reserves memory before admission and evicts unpinned completed entries | `cache::reserves_memory_before_admission_and_evicts_unpinned_completed_entries` |
| `media-cache`: preserves reference-counted pins and rejects writes under pressure | `cache::preserves_reference_counted_pins_and_rejects_writes_under_pressure` |
| `media-cache`: removes a retired session after its final reader releases the pin | `cache::removes_a_retired_session_after_its_final_reader_releases_the_pin` |
| `media-cache`: caps allocated disk bytes and removes only its owned directory | `cache::caps_allocated_disk_bytes_and_removes_only_its_owned_directory` |
| `media-cache`: keeps validated bytes in bounded memory when its disk write fails | `cache::keeps_validated_bytes_in_bounded_memory_when_its_disk_write_fails` |
| `media-cache`: preserves the free-space reserve without losing foreground bytes | `cache::preserves_the_free_space_reserve_without_losing_foreground_bytes` |
| `media-proxy`: serves exact byte ranges assembled from parallel upstream requests | `proxy::serves_exact_byte_ranges_assembled_from_parallel_upstream_requests` |
| `media-proxy`: abandons silently stalled upstream requests and still delivers every byte | `proxy::abandons_silently_stalled_upstream_requests_and_still_delivers_every_byte` |
| `media-proxy`: recovers a throttled probe within the same playback request | `proxy::recovers_a_throttled_probe_within_the_same_playback_request` |
| `media-proxy`: refreshes an expired redirect target once instead of per range | `proxy::refreshes_an_expired_redirect_target_once_instead_of_per_range` |
| `media-proxy`: uses one connection when the player has plenty buffered and several when it is short | `proxy::uses_one_connection_when_the_player_has_plenty_buffered_and_several_when_it_is_short` |
| `media-proxy`: streams the first bytes before a range completes and resumes a dropped range | `proxy::streams_the_first_bytes_before_a_range_completes_and_resumes_a_dropped_range` |
| `media-proxy`: warms the opening and tail and reuses both when playback opens the same source | `proxy::warms_the_opening_and_tail_and_reuses_both_when_playback_opens_the_same_source` |
| `media-proxy`: cancels an unfinished tail warm-up without retrying it | `proxy::cancels_an_unfinished_tail_warm_up_without_retrying_it` |
| `media-proxy`: cancels a queued tail warm-up within the connection cap | `proxy::cancels_a_queued_tail_warm_up_within_the_connection_cap` |
| `media-proxy`: reuses an in-progress range across a brief gap between player reads | `proxy::reuses_an_in_progress_range_across_a_brief_gap_between_player_reads` |
| `media-proxy`: remembers a source whose server refuses connections and fails fast | `proxy::remembers_a_source_whose_server_refuses_connections_and_fails_fast` |
| `media-proxy`: passes through sources that do not support range requests | `proxy::passes_through_sources_that_do_not_support_range_requests` |
| `media-proxy`: rejects unknown and replaced sessions | `proxy::rejects_unknown_and_replaced_sessions` |
| `playback-buffering`: allows roughly one minute of forward buffer for high-bitrate streams | Excluded: browser-only `@stremio/stremio-video` HLS configuration, not a native policy/proxy contract |

The disk-write failure fixture uses a colliding directory at the hashed filename
instead of Unix chmod, retaining the assertions and running on Windows too.
Proxy fixtures inject the Rust fetch trait; HTTP behavior is also exercised
through actual loopback Hyper servers and reqwest clients. Timing uses paused
Tokio time, with an explicit driver to keep socket I/O and blocking filesystem
work from accidentally advancing the clock to an unrelated timeout.

Added coverage includes stale/live owner locks, missing disk files, closed cache
admission, comfortable buffering, malformed ranges, redirects and credentials,
caller resolver success/failure/size mismatch, refresh budgets, changed validators,
unverified resume, overlong bodies, final-byte validation, virtual HTTP-date waits,
Host/peer/method/token rejection, warm expiry, drop cancellation and unread-client
shutdown. The 40 MiB Hyper end-to-end fixture injects slow chunks, 429 with
Retry-After, 503, a mid-body disconnect and signed-target expiry; sequential reads
and three seeks check every byte and cache budgets. Upstream concurrency is
bounded by the scheduler's cap, asserted at each range admission against the
current policy (refresh probes are separate), with one-connection and urgent tests.

## Preserved behavior and changes

- Preserved: 2 MiB chunks; fixed demand of 6/3/1 at buffering, 20 and 60 seconds;
  opt-in adaptive policy; 48-chunk foreground window; eight readers; seven total
  chunk/probe attempts; eight-second inactivity and 90-second probe limits;
  scaled exponential stall cooldown; Retry-After precedence; 60-second stall-cap
  recovery; five-second rate window; 250 ms reader-gap grace; first three chunks
  plus tail warm-up; ten-minute warm TTL; two-minute unreachable hold; passthrough;
  suffix/clamped ranges and malformed Range fallback; strong validator checks.
- Preserved oddities have comments: write reservations count again for memory
  residency; refreshed media without a validator is refused despite equal size.
- Requested native changes: exclusive owner lock instead of PID probing; safe
  fs4 allocation/free-space queries; strict Host and loopback-peer checks returning
  404; caller resolver and refresh budget; redacted source/session diagnostics;
  URL-free structured events; fully tracked connection shutdown. Adaptive mode
  retains the `PANORAMA_ADAPTIVE_BUFFERING=1` default and can be set explicitly.
- Demonstrated TS bug fixed: exhausted prefetch chunks can be re-created by
  `schedule()`, restarting their retry budget. Rust makes exhausted transfer
  errors terminal. Regression:
  `exhausted_chunk_retries_stop_instead_of_scheduler_restarting_the_failure`
  asserts exactly seven requests and none after advancing ten minutes.
- Demonstrated admission deadlock fixed: with one chunk's memory and unavailable
  disk, an uncacheable completed previous chunk retains its reservation while
  the next foreground chunk waits forever. Once cache pressure disables
  prefetch, Rust releases consumed uncacheable chunks. Regression:
  `memory_only_single_chunk_budget_releases_consumed_uncacheable_chunks`.
- Disk reads additionally discard missing or wrong-length files. Retired sessions
  reject late writes, preventing a cancelled disk write from resurrecting entries.
- Resumed-port correction: free-space admission uses checked subtraction. The
  previous Rust implementation used saturation and admitted an allocation larger
  than available space when the configured reserve was zero. Regression:
  `disk_allocation_rejects_insufficient_free_space_even_without_reserve`.
- Demonstrated validator bug fixed: TS Date.parse accepts ISO dates and even
  `"0"` as Last-Modified validators, generating invalid If-Range values.
  `rejects_non_http_last_modified_values_that_cannot_be_if_range_validators`
  rejects them: [RFC 9110 section 13.1.5](https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1.5)
  requires an entity tag or HTTP-date. Standard and obsolete HTTP dates remain
  supported by httpdate.
- Final bytes wait for upstream EOF/validation, as in TS. A complete prefix whose
  body then stalls is retried from the beginning, also matching TS.
- The Hyper disconnect fixture waits 50 virtual milliseconds after its prefix,
  allowing those bytes to flush before aborting. This makes the end-to-end
  assertion exercise partial resume instead of a pre-body failure. Sequential
  playback uses a one-connection policy so throttling cannot cancel this fault
  as surplus prefetch; the three seeks switch to urgent parallelism.

## Behaviours changed

- Redirect SSRF: source validation rejects loopback, unspecified and link-local
  literal IPs, including IPv4-mapped IPv6. The production reqwest DNS resolver
  filters every lookup's actual connect addresses and rejects empty results with
  sanitized `InvalidDestination`; manual redirects repeat source validation.
  Private LAN addresses remain allowed. System proxies are disabled so remote
  proxy-side DNS cannot bypass this policy. Only a test-only constructor maps
  `upstream.invalid` to the local Hyper fixture. Regressions:
  `address_classifier_blocks_local_destinations_and_preserves_lan_servers`,
  `literal_ip_sources_reject_forbidden_destinations_without_exposing_urls`,
  `dns_answers_drop_forbidden_addresses_and_reject_empty_results`,
  `production_transport_rejects_loopback_dns_with_sanitized_error`, and the
  expanded `manual_redirects_revalidate_each_hop_limit_ten_and_send_no_cookies`.
- Reservation ownership: each allocation receives a unique lease. Chunk drop
  releases only its own lease; a stale owner cannot remove a replacement's
  reservation. Duplicate keys wait instead of sharing another owner's allocation.
  Regression: `old_chunk_drop_preserves_replacement_reservation_and_cache_stats`.
- Impossible admission: cache/proxy construction rejects memory budgets below
  one 2 MiB chunk with a clear `MemoryBudget` error, before cache filesystem work.
  Oversized reservations return sanitized `ChunkTooLarge`; the scheduler propagates
  that terminal error instead of retrying every 25 ms. Temporary pressure still
  waits. Regressions: `cache_and_proxy_reject_memory_budgets_below_one_chunk` and
  `oversized_chunk_admission_is_terminal_and_preserves_budget`; scheduler coverage
  `opening_chunk_larger_than_budget_fails_without_retrying` requires an immediate
  terminal result without advancing time.
- Refresh test correction: `refresh_budget_and_backoff_are_per_session_and_rolling`
  now polls successive refreshes immediately, verifies no resolver call through
  the last millisecond before each 30-second deadline, then verifies the call at
  the deadline. It retains rolling-budget assertions; production refresh behavior
  is unchanged.
- Accept recovery: failed accepts retry after 50 ms instead of permanently
  stopping the server. Stop interrupts that delay. The paused-time regression
  `accept_errors_back_off_resume_serving_and_stop_without_waiting` injects an
  accept reset, checks the retry deadline, serves a real HTTP request on the next
  accepted socket, then stops during another accept failure without advancing time.

No dependencies were added for these fixes.

## Refresh contract

401/403/404/410, status 200 after range mode, a changed validator or a changed
range/total trigger a serialized refresh. Concurrent failures carry a source
generation; once refreshed, failures from the old generation share that result.
There are at most three refresh attempts per rolling ten minutes, separated by
the original 30-second backoff. A supplied `MediaResolver` receives the redacted
`MediaSource` and returns a validated fresh source. Without a resolver the
original URL is fetched again, preserving the TS redirect-resolver behavior.

Before switching, a `bytes=0-0` request must return 206, the original total size,
and the original strong ETag or Last-Modified validator. Equal size alone cannot
prove byte identity. Mismatch, resolver failure, timeout or budget exhaustion is
terminal, visible through `SessionHandle::stats().error`; changed size reports
`Resolved media size changed`. Existing buffers and reader positions survive
successful refresh. Dropping futures/bodies cancels obsolete upstream requests.

## Dependencies and Phase 3.3 calls

Exact pins and reasons are in `native/README.md`: tokio 1.53.2, reqwest 0.12.28
(rustls; defaults disabled), hyper 1.12.0, hyper-util 0.1.21, http-body-util 0.1.5,
http 1.5.0, bytes 1.12.1, futures 0.3.34, sha2 0.10.9, getrandom 0.3.4,
httpdate 1.0.3 and dev-only tempfile 3.27.0 reuse locked versions. fs4 0.13.1 is
the additional safe platform wrapper; its windows-sys 0.59.0 is the only new
transitive package. No unsafe code or native TLS was added to panorama-core.

1. Start `MediaProxy::start(options, Some(resolver)).await` once per player host.
2. Optionally call `set_event_sink` for sanitized transport diagnostics and
   `warm(MediaSource::new(url)?).await` before playback.
3. Call `create_session(source).await`; pass `SessionHandle.url` to MPV. This
   replaces prior playback and adopts a matching warm-up.
4. Feed `SessionHandle::set_read_ahead(BufferingSample)` with monotonic `now_ms`,
   buffered seconds, source Mbps, paused/buffering flags and playback speed.
   Download rate, transfer demand and throttling are measured internally.
5. Poll `stats()` for size, rate, cache accounting and terminal error; use
   `resume_buffer_seconds` (five seconds) for MPV `cache-pause-wait`.
6. On stop or source replacement, await `SessionHandle::close()`. On host exit,
   await `MediaProxy::close()`; default accepted-connection deadline is 250 ms.
   Drop also cancels listeners, connections and upstream tasks.
