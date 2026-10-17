use super::*;

fn fake() -> (Player, Receiver<Request>, Sender<Result<(), MpvError>>) {
    let (commands, incoming) = mpsc::channel();
    let (updates, events) = mpsc::channel();
    let (completed, done) = mpsc::channel();
    (
        Player {
            commands,
            events: Some(events),
            updates,
            done: Some(done),
            stopping: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(Wake::default()),
            terminal: Arc::new(AtomicBool::new(false)),
        },
        incoming,
        completed,
    )
}

#[test]
fn close_is_nonblocking_delivers_shutdown_once_and_disconnects() {
    let (mut player, incoming, completed) = fake();
    let events = player.events().unwrap();
    assert_eq!(player.events().unwrap_err(), MpvError::EventsTaken);
    player
        .load("https://private.invalid/x?token=secret")
        .unwrap();
    assert!(matches!(incoming.recv().unwrap(), Request::Command(_)));
    let close = player.close();
    completed.send(Ok(())).unwrap();
    assert_eq!(close.recv_timeout(Duration::from_secs(1)).unwrap(), Ok(()));
    assert_eq!(
        events.recv_timeout(Duration::from_secs(1)).unwrap(),
        PlayerEvent::Shutdown
    );
    assert!(events.recv_timeout(Duration::from_secs(1)).is_err());
    assert!(incoming.recv_timeout(Duration::from_secs(1)).is_err());
}

#[test]
fn scalars_and_convenience_arguments_are_validated() {
    let (player, _, completed) = fake();
    assert_eq!(player.set_volume(f64::NAN), Err(MpvError::InvalidArgument));
    assert_eq!(
        player.seek(-1.0, SeekMode::Absolute),
        Err(MpvError::InvalidArgument)
    );
    assert_eq!(
        player.set_audio_track(Some(0)),
        Err(MpvError::InvalidArgument)
    );
    assert_eq!(player.command(&[]), Err(MpvError::InvalidArgument));
    assert_eq!(player.load("secret\0url"), Err(MpvError::InvalidArgument));
    assert_eq!(
        player.set_property("speed", f64::INFINITY),
        Err(MpvError::InvalidArgument)
    );
    completed.send(Ok(())).unwrap();
}

#[test]
fn shutdown_notification_is_idempotent() {
    let (send, recv) = mpsc::channel();
    let terminal = AtomicBool::new(false);
    shutdown_event(&send, &terminal);
    shutdown_event(&send, &terminal);
    assert_eq!(
        recv.try_iter().collect::<Vec<_>>(),
        vec![PlayerEvent::Shutdown]
    );
}

#[test]
fn timeout_does_not_claim_actual_shutdown() {
    let (mut player, _incoming, completed) = fake();
    let events = player.events().unwrap();
    let result = player.begin_close_with_timeout(Duration::from_millis(10));
    assert_eq!(
        result.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(MpvError::ShutdownTimeout)
    );
    assert_eq!(
        events.recv_timeout(Duration::from_secs(1)).unwrap(),
        PlayerEvent::Error(MpvError::ShutdownTimeout)
    );
    assert!(!player.terminal.load(Ordering::Acquire));
    assert!(events.try_recv().is_err());
    shutdown_event(&player.updates, &player.terminal);
    assert_eq!(events.try_recv().unwrap(), PlayerEvent::Shutdown);
    drop(completed);
}

#[test]
fn drop_uses_the_same_nonblocking_shutdown_path() {
    let (mut player, _incoming, completed) = fake();
    let events = player.events().unwrap();
    completed.send(Ok(())).unwrap();
    drop(player);
    assert_eq!(
        events.recv_timeout(Duration::from_secs(1)).unwrap(),
        PlayerEvent::Shutdown
    );
}
