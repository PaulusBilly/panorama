//! The party's network side, on the services runtime. One task owns the endpoint and the
//! session; each connection has a reader task and a writer task. The GPUI side talks to it
//! through [`Input`] and hears back through [`Event`]. Dropping the inputs leaves the party.
use super::{EndReason, InviteLink, Link, PartySync, PartyView, Role, RosterRow, trace};
use crate::playback::host::Command;
use futures::channel::mpsc as gpui_mpsc;
use panorama_core::{
    party::{
        Action, ConnId, Down, HostInput, HostOutput, HostSession, HostSettings, MAX_MEMBERS,
        MemberInput, MemberOutput, MemberSession, MemberState, MemberView, Notice, NoticeKind,
        PartyMedia, PlaybackState, TrackChoice, decode_down, decode_up, encode,
        transport::{
            Invite, PartyConn, PartyEndpoint, PartyPath, PartyReceiver, PartySecret, PartySender,
            TransportError,
        },
    },
    store::{Key, Store},
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    runtime::Handle,
    sync::{
        OwnedSemaphorePermit, Semaphore,
        mpsc::{self, UnboundedReceiver, UnboundedSender, unbounded_channel},
    },
    task::{JoinHandle, JoinSet},
};

const TICK: Duration = Duration::from_millis(250);
/// How long a goodbye waits for the other side to close before the connection goes.
const GRACE: Duration = Duration::from_secs(2);
/// The host moves its own player only when it is this far off the party's timeline.
const HOST_SLACK: f64 = 0.5;
const KEY: &str = "party.key";
/// Lines a peer may leave unread before it is dropped.
const QUEUE: usize = 256;
/// Lines a second a member may send, after a burst of [`BURST`]: a ping every 2 s and a report
/// a second leave room for the fastest key repeat on seek.
const RATE: f64 = 30.0;
const BURST: f64 = 60.0;
/// Connections the host holds at once, joined or still shaking hands.
const MAX_PEERS: usize = MAX_MEMBERS * 2;

/// From the GPUI side.
#[derive(Clone, Debug)]
pub(super) enum Input {
    /// The user's own play, pause or seek.
    Local(Action),
    /// The local player, on change and while playing.
    Report {
        state: MemberState,
        position: f64,
        duration: Option<f64>,
    },
    /// Host: the local player opened this media at `at` seconds.
    Media {
        media: PartyMedia,
        at: f64,
    },
    /// Member: whether the playing source is the host's release.
    Release {
        matched: bool,
    },
    /// Member: the player opened a new file.
    Reopened,
    Settings(HostSettings),
    /// Host: the local player's audio and subtitle tracks.
    Tracks(TrackChoice),
    Sync(PartySync),
    /// Host: leave the lobby, so the party plays once everyone is ready.
    Start,
    /// Host: the film closed; everyone waits in the lobby for the next one.
    Stop,
}

/// To the GPUI side.
pub(super) enum Event {
    View(PartyView),
    /// Apply to the playback host as is: these never count as the user's actions.
    Player(Command),
    /// Member: open this media.
    Load(PartyMedia),
    /// Member: the host's track choice.
    Tracks(TrackChoice),
    /// Member: the host closed the film.
    Stop,
}

pub(super) struct Spawned {
    pub inputs: UnboundedSender<Input>,
    pub events: gpui_mpsc::UnboundedReceiver<Event>,
}

pub(super) fn host(
    runtime: &Handle,
    store: Option<Arc<Store>>,
    name: String,
    settings: HostSettings,
    sync: PartySync,
) -> Spawned {
    spawn(runtime, move |inputs, events| async move {
        Host::run(store, name, settings, sync, inputs, events).await
    })
}

pub(super) fn member(
    runtime: &Handle,
    store: Option<Arc<Store>>,
    name: String,
    invite: Invite,
    sync: PartySync,
) -> Spawned {
    spawn(runtime, move |inputs, events| async move {
        run_member(store, name, invite, sync, inputs, events).await
    })
}

fn spawn<F, Fut>(runtime: &Handle, run: F) -> Spawned
where
    F: FnOnce(UnboundedReceiver<Input>, Events) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (inputs, receiver) = unbounded_channel();
    let (sender, events) = gpui_mpsc::unbounded();
    // Not a `Job`: leaving must not cut the goodbye short, and the task ends by itself once
    // the inputs close.
    runtime.spawn(run(receiver, Events::new(sender)));
    Spawned { inputs, events }
}

/// Sends the view only when it changed.
struct Events {
    sender: gpui_mpsc::UnboundedSender<Event>,
    shown: Option<PartyView>,
}

impl Events {
    fn new(sender: gpui_mpsc::UnboundedSender<Event>) -> Self {
        Self {
            sender,
            shown: None,
        }
    }

    fn send(&self, event: Event) {
        let _ = self.sender.unbounded_send(event);
    }

    fn player(&self, command: Command) {
        trace(|| format!("apply {}", describe(&command)));
        self.send(Event::Player(command));
    }

    fn view(&mut self, view: &PartyView) {
        if self.shown.as_ref() != Some(view) {
            self.shown = Some(view.clone());
            self.send(Event::View(view.clone()));
        }
    }
}

fn describe(command: &Command) -> String {
    match command {
        Command::SetPaused(paused) => format!("paused={paused}"),
        Command::Seek(to) => format!("seek={to:.3}"),
        Command::SetSpeed(speed) => format!("speed={speed:.3}"),
        _ => "other".into(),
    }
}

async fn bind(store: Option<&Store>) -> Result<PartyEndpoint, TransportError> {
    let key = Key::pref(KEY).ok();
    let stored = key
        .as_ref()
        .and_then(|key| store?.get(key).ok().flatten())
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());
    let (endpoint, bytes) = PartyEndpoint::bind(stored).await?;
    if stored != Some(bytes)
        && let (Some(store), Some(key)) = (store, key)
    {
        let _ = store.set(&key, &bytes);
    }
    Ok(endpoint)
}

/// One connection's writer: lines in order, then a graceful close.
pub(super) enum Out {
    Line(String),
    Close,
}

/// The lines waiting for one connection's writer, at most [`QUEUE`] of them, so a peer that
/// stops reading costs a bounded amount of memory before it is dropped.
pub(super) struct Queue(mpsc::Sender<Out>);

/// The peer left [`QUEUE`] lines unread, or its writer is gone.
#[derive(Debug)]
pub(super) struct Stalled;

impl Queue {
    pub(super) fn new() -> (Self, mpsc::Receiver<Out>) {
        let (sender, receiver) = mpsc::channel(QUEUE);
        (Self(sender), receiver)
    }

    pub(super) fn line(&self, line: String) -> Result<(), Stalled> {
        self.0.try_send(Out::Line(line)).map_err(|_| Stalled)
    }

    /// Asks the writer to say goodbye after what is queued; `false` when it can't take that.
    fn close(&self) -> bool {
        self.0.try_send(Out::Close).is_ok()
    }
}

/// How fast one connection may send: [`RATE`] lines a second, with [`BURST`] to spare.
#[derive(Debug)]
pub(super) struct RateLimit {
    tokens: f64,
    at: Instant,
}

impl RateLimit {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            tokens: BURST,
            at: now,
        }
    }

    /// Spends a line's worth; `false` once the connection sends faster than it may.
    pub(super) fn allow(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.at = now;
        self.tokens = (self.tokens + elapsed * RATE).min(BURST);
        let allowed = self.tokens >= 1.0;
        if allowed {
            self.tokens -= 1.0;
        }
        allowed
    }
}

fn writer(runtime: &Handle, mut sender: PartySender) -> (Queue, JoinHandle<()>) {
    let (lines, mut receiver) = Queue::new();
    let task = runtime.spawn(async move {
        while let Some(out) = receiver.recv().await {
            match out {
                Out::Line(line) => {
                    if sender.send(&line).await.is_err() {
                        return;
                    }
                }
                Out::Close => break,
            }
        }
        sender.finish(GRACE).await;
    });
    (lines, task)
}

/// Lines from one connection, then `None` once it ends, breaks, or sends faster than `limit`
/// allows.
fn reader<T: Copy + Send + 'static>(
    runtime: &Handle,
    mut receiver: PartyReceiver,
    tag: T,
    mut limit: Option<RateLimit>,
    lines: UnboundedSender<(T, Option<String>)>,
) -> JoinHandle<()> {
    runtime.spawn(async move {
        while let Ok(Some(line)) = receiver.recv().await {
            if limit
                .as_mut()
                .is_some_and(|limit| !limit.allow(Instant::now()))
            {
                trace(|| "peer over its rate".into());
                break;
            }
            if lines.send((tag, Some(line))).is_err() {
                return;
            }
        }
        let _ = lines.send((tag, None));
    })
}

/// Hands over connections once their handshake is done, each with its slot among
/// [`MAX_PEERS`]. While every slot is taken, newcomers are refused before any handshake.
async fn acceptor(
    endpoint: PartyEndpoint,
    opened: UnboundedSender<(PartyConn, OwnedSemaphorePermit)>,
) {
    let slots = Arc::new(Semaphore::new(MAX_PEERS));
    let mut handshakes = JoinSet::new();
    loop {
        tokio::select! {
            joining = endpoint.accept() => {
                let Some(joining) = joining else {
                    return;
                };
                match slots.clone().try_acquire_owned() {
                    Ok(slot) => {
                        handshakes.spawn(async move {
                            joining.handshake().await.map(|conn| (conn, slot))
                        });
                    }
                    Err(_) => joining.refuse(),
                }
            }
            Some(done) = handshakes.join_next() => {
                if let Ok(Ok(peer)) = done
                    && opened.send(peer).is_err()
                {
                    return;
                }
            }
        }
    }
}

struct Peer {
    out: Queue,
    path: PartyPath,
    member: Option<u32>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
    _slot: OwnedSemaphorePermit,
}

struct Host {
    session: HostSession,
    view: PartyView,
    events: Events,
    peers: BTreeMap<ConnId, Peer>,
    /// Writers of dropped peers, still saying goodbye.
    closing: Vec<JoinHandle<()>>,
    origin: Instant,
    /// The host's own player: state, position, and when it said so.
    local: Option<(MemberState, f64, i64)>,
}

impl Host {
    async fn run(
        store: Option<Arc<Store>>,
        name: String,
        settings: HostSettings,
        sync: PartySync,
        mut inputs: UnboundedReceiver<Input>,
        mut events: Events,
    ) {
        let mut view = PartyView::new(
            Role::Host {
                invite: None,
                settings,
            },
            Link::Starting,
        );
        view.anyone_controls = settings.anyone_controls;
        view.sync = sync;
        events.view(&view);
        let started = async {
            let endpoint = bind(store.as_deref()).await?;
            let secret = PartySecret::generate()?;
            let invite = endpoint.invite(&secret).await?;
            Ok::<_, TransportError>((endpoint, secret, invite))
        };
        let (endpoint, secret, invite) = match started.await {
            Ok(started) => started,
            Err(error) => {
                trace(|| format!("host failed {error:?}"));
                view.link = Link::Ended(EndReason::Failed(error));
                events.view(&view);
                return;
            }
        };
        view.role = Role::Host {
            invite: Some(InviteLink(invite)),
            settings,
        };
        view.link = Link::Live;
        trace(|| "host live".into());
        let mut host = Host {
            session: HostSession::new(name, secret, settings),
            view,
            events,
            peers: BTreeMap::new(),
            closing: Vec::new(),
            origin: Instant::now(),
            local: None,
        };
        host.events.view(&host.view);
        let runtime = Handle::current();
        let (opened, mut accepted) = unbounded_channel();
        let acceptor = runtime.spawn(acceptor(endpoint.clone(), opened));
        let (lines, mut incoming) = unbounded_channel::<(ConnId, Option<String>)>();
        let mut next_conn = 0u32;
        let mut tick = tokio::time::interval(TICK);
        loop {
            tokio::select! {
                input = inputs.recv() => match input {
                    Some(input) => host.input(input),
                    None => break,
                },
                Some((conn, slot)) = accepted.recv() => {
                    let id = ConnId(next_conn);
                    next_conn = next_conn.wrapping_add(1);
                    let (sender, receiver) = conn.split();
                    let path = sender.path_handle();
                    let (out, writer) = writer(&runtime, sender);
                    let limit = Some(RateLimit::new(Instant::now()));
                    let reader = reader(&runtime, receiver, id, limit, lines.clone());
                    host.peers.insert(id, Peer { out, path, member: None, reader, writer, _slot: slot });
                    host.handle(HostInput::Connected { conn: id });
                }
                Some((conn, line)) = incoming.recv() => match line.map(|line| decode_up(&line)) {
                    Some(Ok(msg)) => host.handle(HostInput::Message { conn, msg }),
                    Some(Err(_)) => {
                        host.handle(HostInput::Disconnected { conn });
                        host.drop_peer(conn);
                    }
                    None => {
                        if host.peers.contains_key(&conn) {
                            host.handle(HostInput::Disconnected { conn });
                            host.drop_peer(conn);
                        }
                    }
                },
                _ = tick.tick() => {
                    host.handle(HostInput::Tick);
                    host.refresh_paths();
                    host.closing.retain(|writer| !writer.is_finished());
                }
            }
            host.events.view(&host.view);
        }
        host.handle(HostInput::End);
        host.view.link = Link::Ended(EndReason::Left);
        host.events.view(&host.view);
        acceptor.abort();
        for conn in host.peers.keys().copied().collect::<Vec<_>>() {
            host.drop_peer(conn);
        }
        for writer in std::mem::take(&mut host.closing) {
            let _ = tokio::time::timeout(GRACE + Duration::from_secs(1), writer).await;
        }
        endpoint.close().await;
        trace(|| "host closed".into());
    }

    fn now(&self) -> i64 {
        self.origin.elapsed().as_millis() as i64
    }

    fn input(&mut self, input: Input) {
        let now = self.now();
        match input {
            Input::Local(action) => {
                trace(|| format!("host local {action:?}"));
                self.handle(HostInput::Local(action));
            }
            Input::Report {
                state,
                position,
                duration,
            } => {
                trace(|| format!("report state={state:?} pos={position:.3}"));
                self.local = Some((state, position, now));
                self.handle(HostInput::LocalReport {
                    state,
                    position,
                    duration,
                });
            }
            Input::Media { media, at } => {
                trace(|| {
                    format!(
                        "host media film={} video={} at={at:.3}",
                        media.film_id, media.video_id
                    )
                });
                self.handle(HostInput::SetMedia { media, at });
            }
            Input::Start => {
                trace(|| "host start".into());
                self.handle(HostInput::Start);
            }
            Input::Stop => {
                trace(|| "host stop".into());
                self.handle(HostInput::Stop);
            }
            Input::Settings(settings) => {
                if let Role::Host {
                    settings: shown, ..
                } = &mut self.view.role
                {
                    *shown = settings;
                }
                self.view.anyone_controls = settings.anyone_controls;
                self.handle(HostInput::Settings(settings));
            }
            Input::Tracks(tracks) => {
                trace(|| format!("host tracks {tracks:?}"));
                self.handle(HostInput::Tracks(tracks));
            }
            Input::Sync(sync) => self.view.sync = sync,
            Input::Release { .. } | Input::Reopened => {}
        }
    }

    fn handle(&mut self, input: HostInput) {
        let now = self.now();
        let mut stalled_peers = Vec::new();
        for output in self.session.handle(now, input) {
            match output {
                HostOutput::Send { conn, msg } => {
                    if let Down::Welcome { member_id } = msg
                        && let Some(peer) = self.peers.get_mut(&conn)
                    {
                        trace(|| format!("host welcomed member_id={member_id}"));
                        peer.member = Some(member_id);
                    }
                    let stalled = self
                        .peers
                        .get(&conn)
                        .is_some_and(|peer| peer.out.line(encode(&msg)).is_err());
                    if stalled {
                        trace(|| "peer stopped reading".into());
                        stalled_peers.push(conn);
                    }
                }
                HostOutput::Drop { conn } => self.drop_peer(conn),
                HostOutput::Apply(playback) => self.apply(now, playback),
                HostOutput::Roster(members) => self.roster(members),
                HostOutput::Notice(notice) => {
                    trace(|| format!("notice {:?}", notice.kind));
                    self.view.notify(notice);
                }
            }
        }
        self.view.gate = Some(self.session.gate());
        for conn in stalled_peers {
            if self.peers.contains_key(&conn) {
                self.drop_peer(conn);
                self.handle(HostInput::Disconnected { conn });
            }
        }
    }

    /// Moves the host's own player onto the party's timeline, seeking only when it is off.
    fn apply(&self, now: i64, playback: PlaybackState) {
        let expected = playback.expected_at(now);
        let here = self.local.map(|(state, position, at)| {
            if state == MemberState::Playing {
                position + (now - at) as f64 / 1000.0
            } else {
                position
            }
        });
        if here.is_none_or(|here| (here - expected).abs() > HOST_SLACK) {
            self.events.player(Command::Seek(expected));
        }
        self.events.player(Command::SetPaused(!playback.playing));
    }

    fn roster(&mut self, members: Vec<MemberView>) {
        self.view.roster = members
            .into_iter()
            .map(|member| RosterRow { member, path: None })
            .collect();
        self.refresh_paths();
    }

    fn refresh_paths(&mut self) {
        for row in &mut self.view.roster {
            row.path = self
                .peers
                .values()
                .find(|peer| peer.member == Some(row.member.member_id) && !row.member.is_host)
                .and_then(|peer| peer.path.get())
                .map(|status| status.kind);
        }
    }

    fn drop_peer(&mut self, conn: ConnId) {
        if let Some(peer) = self.peers.remove(&conn) {
            peer.reader.abort();
            if !peer.out.close() {
                peer.writer.abort();
            }
            self.closing.push(peer.writer);
        }
    }
}

async fn run_member(
    store: Option<Arc<Store>>,
    name: String,
    invite: Invite,
    sync: PartySync,
    mut inputs: UnboundedReceiver<Input>,
    mut events: Events,
) {
    let mut view = PartyView::new(Role::Member, Link::Starting);
    view.sync = sync;
    events.view(&view);
    let secret = invite.secret.clone();
    let connected = async {
        let endpoint = bind(store.as_deref()).await?;
        view.link = Link::Connecting;
        events.view(&view);
        let conn = endpoint.connect(&invite).await?;
        Ok::<_, TransportError>((endpoint, conn))
    };
    let (endpoint, conn) = match connected.await {
        Ok(connected) => connected,
        Err(error) => {
            trace(|| format!("member failed {error:?}"));
            view.link = Link::Ended(EndReason::Failed(error));
            events.view(&view);
            return;
        }
    };
    let runtime = Handle::current();
    let (sender, receiver) = conn.split();
    let path = sender.path_handle();
    let (out, writer_task) = writer(&runtime, sender);
    let (lines, mut incoming) = unbounded_channel::<((), Option<String>)>();
    let reader_task = reader(&runtime, receiver, (), None, lines);
    let mut member = Member {
        session: MemberSession::new(name, secret),
        view,
        events,
        out,
        origin: Instant::now(),
        done: false,
    };
    member.handle(MemberInput::Start);
    let mut tick = tokio::time::interval(TICK);
    while !member.done {
        tokio::select! {
            input = inputs.recv() => match input {
                Some(input) => member.input(input),
                None => {
                    member.handle(MemberInput::Leave);
                    member.end(EndReason::Left);
                }
            },
            Some(((), line)) = incoming.recv() => match line.map(|line| decode_down(&line)) {
                Some(Ok(msg)) => member.handle(MemberInput::Down(msg)),
                Some(Err(_)) | None => {
                    member.handle(MemberInput::Leave);
                    member.end(EndReason::Lost);
                }
            },
            _ = tick.tick() => {
                member.handle(MemberInput::Tick);
                let kind = path.get().map(|status| status.kind);
                for row in &mut member.view.roster {
                    row.path = if row.member.is_host { kind } else { None };
                }
            }
        }
        member.events.view(&member.view);
    }
    reader_task.abort();
    if !member.out.close() {
        writer_task.abort();
    }
    let _ = tokio::time::timeout(GRACE + Duration::from_secs(1), writer_task).await;
    endpoint.close().await;
    trace(|| "member closed".into());
}

struct Member {
    session: MemberSession,
    view: PartyView,
    events: Events,
    out: Queue,
    origin: Instant,
    done: bool,
}

impl Member {
    fn input(&mut self, input: Input) {
        match input {
            Input::Local(action) if self.view.anyone_controls => {
                trace(|| format!("member request {action:?}"));
                self.handle(MemberInput::Local(action));
            }
            Input::Local(_) => {
                trace(|| "member denied locally".into());
                let by = self.view.host_name();
                self.view.notify(Notice {
                    kind: NoticeKind::Denied,
                    by,
                    at: None,
                });
            }
            Input::Report {
                state,
                position,
                duration,
            } => {
                trace(|| format!("report state={state:?} pos={position:.3}"));
                self.handle(MemberInput::Player {
                    state,
                    position,
                    duration,
                });
            }
            Input::Release { matched } => {
                trace(|| format!("member release matched={matched}"));
                self.handle(MemberInput::Release { matched });
            }
            Input::Reopened => {
                trace(|| "member reopened".into());
                self.handle(MemberInput::Reopened);
            }
            Input::Sync(sync) => self.view.sync = sync,
            Input::Media { .. }
            | Input::Settings(_)
            | Input::Tracks(_)
            | Input::Start
            | Input::Stop => {}
        }
    }

    fn handle(&mut self, input: MemberInput) {
        let now = self.origin.elapsed().as_millis() as i64;
        for output in self.session.handle(now, input) {
            match output {
                MemberOutput::Send(up) => {
                    if self.out.line(encode(&up)).is_err() {
                        trace(|| "host stopped reading".into());
                        self.end(EndReason::Lost);
                    }
                }
                MemberOutput::Welcomed { member_id } => {
                    trace(|| format!("member joined member_id={member_id}"));
                    self.view.link = Link::Live;
                    self.view.me = Some(member_id);
                }
                MemberOutput::Load(media) => {
                    trace(|| {
                        format!(
                            "member load film={} video={}",
                            media.film_id, media.video_id
                        )
                    });
                    self.events.send(Event::Load(media));
                }
                MemberOutput::SetPaused(paused) => self.events.player(Command::SetPaused(paused)),
                MemberOutput::Seek(to) => self.events.player(Command::Seek(to)),
                MemberOutput::SetSpeed(speed) => self.events.player(Command::SetSpeed(speed)),
                MemberOutput::Controls { anyone_controls } => {
                    self.view.anyone_controls = anyone_controls;
                }
                MemberOutput::Tracks(tracks) => {
                    trace(|| format!("member tracks {tracks:?}"));
                    self.events.send(Event::Tracks(tracks));
                }
                MemberOutput::Gate(gate) => {
                    trace(|| format!("member gate {gate:?}"));
                    self.view.gate = Some(gate);
                }
                MemberOutput::Roster(members) => {
                    self.view.roster = members
                        .into_iter()
                        .map(|member| RosterRow { member, path: None })
                        .collect();
                }
                MemberOutput::Notice(notice) => {
                    trace(|| format!("notice {:?}", notice.kind));
                    self.view.notify(notice);
                }
                MemberOutput::Stop => {
                    trace(|| "member stop".into());
                    self.events.send(Event::Stop);
                }
                MemberOutput::Ended(reason) => {
                    trace(|| format!("member ended {reason:?}"));
                    self.end(EndReason::Bye(reason));
                }
            }
        }
    }

    /// The first ending sticks: the host's goodbye, then the stream closing, is a goodbye.
    fn end(&mut self, reason: EndReason) {
        if !self.done {
            self.done = true;
            self.view.link = Link::Ended(reason);
        }
    }
}
