# Watch party live test: two machines, two Claudes

A seek and sync test run on two real machines. On each machine a person drives the app and a
Claude Code session in this repo watches that machine's party trace. Hand this file to the
Claude on each machine and say which role it has.

- **Windows = HOST.** It starts the party and does most of the seeking.
- **Mac = MEMBER.** It joins with the invite link.

The fix under test is commit `21e5625` ("Never start or resume a watch party without its host").
Before that fix, a host slower than its friends was given up on after 20 s, so the Mac played
alone and was later pulled back to the host's spot.

---

## 0. Ground rules for each Claude

- You are an observer and log reader. The person drives the app; never synthesize clicks or
  key presses. On Windows, simulated input landed on other windows.
- Read only this machine's log. The other machine's log arrives at the end (step 5).
- Write what you see, with `wall_ms` timestamps, to `party-logs/<role>-notes.md` as you go.
  The heading for each scenario is its id, for example `## S1`.
- Don't change code during the run. Note suspected bugs with the log lines that show them.
  The fixes come after both logs are merged.
- When the person says **"mark <text>"**, append `MARK <wall_ms now> <text>` to your notes.
  These marks are how the two timelines are lined up afterwards.

## 1. Setup (both machines)

```sh
git pull                      # must include 21e5625
git log --oneline -1
mkdir -p party-logs
grep -qx 'party-logs/' .git/info/exclude || echo 'party-logs/' >> .git/info/exclude   # keep logs out of git
```

Build a release binary. Don't test the installed app, because it auto-updates to the last
published release, which doesn't have the fix.

- **Windows:** `pwsh scripts/stage-windows-libmpv.ps1` (once), then `cargo build --release -p panorama-app`.
- **Mac:** `python3 scripts/build-macos-libmpv.py && scripts/stage-macos-libmpv.sh` (once), then `cargo build --release -p panorama-app`.

**Clock offset.** `wall_ms` comes from each machine's own clock, so record how far each clock
is off before starting:

- Windows: `w32tm /stripchart /computer:time.windows.com /samples:3 /dataonly`
- Mac: `sntp time.apple.com`

Put the offsets at the top of the notes file. Anything over about 200 ms must be subtracted
when merging.

## 2. Launch with tracing

Launch the app in the background with stderr going to a log, so you can keep reading it.

**Windows (host), PowerShell:**

```powershell
$env:PANORAMA_PARTY_TRACE = '1'
$log = "party-logs\win-$(Get-Date -Format yyyyMMdd-HHmmss).log"
Start-Process target\release\panorama.exe -RedirectStandardError $log -PassThru
```

**Mac (member):**

```sh
log=party-logs/mac-$(date +%Y%m%d-%H%M%S).log
PANORAMA_PARTY_TRACE=1 PANORAMA_LIBMPV_DIR=$PWD/target/libmpv \
  ./target/release/panorama 2> "$log" &
```

The person then starts a party on Windows, sends the invite link to the Mac, and joins with it.
To join without clicking, the Mac can launch with `--party-join '<link>'` instead.

**Following the log.** Use Monitor or a background tail filtered to the party lines, for example
`tail -f "$log" | grep --line-buffered 'party wall_ms'`. On Windows, use
`Get-Content $log -Wait | Select-String 'party wall_ms'`.

## 3. What the trace lines mean

Every line looks like `party wall_ms=<unix ms> <event>`.

| line | where | meaning |
| --- | --- | --- |
| `host local <Action>` | host | The host's own play, pause or seek went into the party. |
| `member request <Action>` | member | The member asked the host for play, pause or seek. |
| `report state=<S> pos=<p>` | both | This player's state as the party hears it: Loading, Buffering, Paused, Playing, Failed or Away. |
| `player state=<S> pos= paused= seeking= cache= status=` | both | Detail for a report that isn't Playing: is mpv still seeking, and how much is buffered. |
| `member report conn=<n> state=<S> pos=<p>` | host | What the host heard from a member. |
| `apply paused=<b>` / `apply seek=<t>` / `apply speed=<x>` | both | The party moved this player. Speed is drift correction. |
| `notice <Kind>` | both | A toast, for example Seeked, Waiting or Late. |
| `member gate <Gate>` | member | Why the member is held: waiting for whom, and so on. |
| `host start` / `host stop` / `member stop` / `member ended` | both | Party lifecycle. |

A healthy seek by the host goes like this:

1. On the host, `host local Seek(..)`.
2. On both, `apply paused=true` and `apply seek=T`.
3. On both, `report state=Buffering` or `Loading`.
4. On both, `report state=Paused pos≈T`.
5. On both, `apply paused=false`.
6. On both, `report state=Playing`.

Watch for these red flags:

- The member gets `apply paused=false` while the host's latest report is still Buffering or
  Loading, or has no Paused at the new spot. The party started without the host, which is the
  old bug.
- An `apply seek` on the member more than 10 s after the party started playing, back to the
  host's spot. That is the drift yank-back.
- `player state=Buffering ... seeking=true` repeating for more than 30 s, with the same `pos`
  and `cache` near 0. The player is stuck fetching. Note the `status=`.
- `apply speed` that never returns to `1.000`.
- Repeated `rejoin` lines or `member ended`. The connection dropped.

## 4. Scenarios

Run them in order. Before each one, the person says "mark S<n> start" on both machines. After
it, they say "mark S<n> end" and describe what they saw on screen, which goes in the notes.
Pick a film long enough to seek far: torrents show the stall cases best, HTTP streams the
fast cases.

| id | who does what | expected |
| --- | --- | --- |
| S1 | Play for 1 minute, then the host seeks about 20 min ahead with the seek bar. This is the original report. | Both pause. The party starts only after the host reports Paused there, even if that takes more than 20 s. The Mac shows it is waiting for the host. No yank-back afterwards. |
| S2 | The host presses → five times quickly while playing. | One settled spot about 50 s ahead, with no ping-pong between spots. Note how many `host local Seek` lines appear: an earlier run saw rapid presses ignored. |
| S3 | The host seeks backwards about 5 min, into already-watched video. | The player may still have it buffered, so the restart is quick, under 3 s. |
| S4 | The member seeks with the seek bar. "Anyone controls" must be on; otherwise expect `member denied locally`. | `member request Seek` on the Mac, then the same flow as S1 driven by the host. |
| S5 | The host seeks, then presses Pause while everyone waits. | The party stays paused after loading and does not start by itself. |
| S6 | Slow member: on the Mac, enable Network Link Conditioner (3G) or a hotspot, then the host seeks far. | After 20 s the party starts without the Mac and the host shows the member is late. Once the Mac catches up, it seeks to the live spot and plays in sync. |
| S7 | Mid-film stall: the host seeks far on a slow torrent and lets it play into unbuffered video. | When the host buffers, the member pauses and waits for as long as it takes, every time. It is never left to play alone. |
| S8 | Pause, then seek, then play, with the host doing them quickly in that order. | It ends playing at the new spot, with no stuck pause. |
| S9 | The member quits the app during a wait, relaunches and rejoins. | The rejoin lands at the host's spot. The host is not held forever by a member who left. |
| S10 | The host seeks to 30 s before the end. | Both reach the end together, and the member does not roll into the next episode. |

**Known trade-off to confirm, not a bug.** If the host's own player truly wedges, everyone
waits forever. Play during that wait is ignored. If S1 or S7 shows that, record how long it
waited and what `player state=... status=` was on the host.

## 5. Merging the two timelines

At the end, copy one machine's log and notes to the other (AirDrop, a shared folder or a
gist), then ask that machine's Claude:

> Merge `party-logs/win-*.log` and `party-logs/mac-*.log` into `party-logs/merged.log`. Keep
> only `party wall_ms` lines and prefix each with `W` or `M`. Subtract the recorded clock
> offsets, then sort by `wall_ms`. For each scenario between its MARK lines, write in
> `party-logs/findings.md`: expected, observed, verdict (pass, fail or unclear) and the
> evidence lines. List the failures first.

A quick merge by hand, when the offsets are negligible:

```sh
{ sed 's/^/W /' party-logs/win-*.log; sed 's/^/M /' party-logs/mac-*.log; } \
  | grep 'party wall_ms' | sort -t= -k2 -n > party-logs/merged.log
```

## 6. Report back

Bring `findings.md` and both raw logs to the Windows session. Each failing scenario becomes a
regression test in `crates/panorama-core/src/party/tests/` before it gets a fix.
