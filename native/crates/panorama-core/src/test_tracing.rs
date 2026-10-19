//! One process-wide tracing subscriber shared by tests that must observe events on
//! threads they do not own. `tracing` accepts a single global default per process, so
//! each test swaps its subscriber in behind a lock instead of installing its own.
use std::sync::{Mutex, MutexGuard, Once, RwLock};
use tracing::{
    Dispatch, Event, Metadata, Subscriber,
    level_filters::LevelFilter,
    span::{Attributes, Id, Record},
    subscriber::Interest,
};

static INSTALL: Once = Once::new();
static EXCLUSIVE: Mutex<()> = Mutex::new(());
static CURRENT: RwLock<Option<Dispatch>> = RwLock::new(None);

/// Routes global events to `subscriber` until the guard drops; tests holding a guard run one at a time.
pub(crate) fn capture(subscriber: impl Subscriber + Send + Sync + 'static) -> Guard {
    INSTALL.call_once(|| {
        tracing::subscriber::set_global_default(Forward).expect("no other global subscriber");
    });
    let exclusive = EXCLUSIVE.lock().unwrap_or_else(|p| p.into_inner());
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = Some(Dispatch::new(subscriber));
    Guard {
        _exclusive: exclusive,
    }
}

pub(crate) struct Guard {
    _exclusive: MutexGuard<'static, ()>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

fn with<T>(default: T, f: impl FnOnce(&Dispatch) -> T) -> T {
    match CURRENT.read().unwrap_or_else(|p| p.into_inner()).as_ref() {
        Some(dispatch) => f(dispatch),
        None => default,
    }
}

struct Forward;

impl Subscriber for Forward {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        // Never cache a verdict: the inner subscriber changes between tests.
        Interest::sometimes()
    }
    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::TRACE)
    }
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        with(false, |d| d.enabled(metadata))
    }
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        with(Id::from_u64(1), |d| d.new_span(attributes))
    }
    fn record(&self, id: &Id, values: &Record<'_>) {
        with((), |d| d.record(id, values))
    }
    fn record_follows_from(&self, span: &Id, follows: &Id) {
        with((), |d| d.record_follows_from(span, follows))
    }
    fn event(&self, event: &Event<'_>) {
        with((), |d| d.event(event))
    }
    fn enter(&self, id: &Id) {
        with((), |d| d.enter(id))
    }
    fn exit(&self, id: &Id) {
        with((), |d| d.exit(id))
    }
}
