use std::{
    sync::{OnceLock, mpsc},
    thread,
};

static LOG: OnceLock<mpsc::Sender<Option<String>>> = OnceLock::new();

/// A background diagnostic sink, drained after the GPUI event loop exits.
pub struct Logger {
    sender: mpsc::Sender<Option<String>>,
    worker: Option<thread::JoinHandle<()>>,
}

/// Start the logger before opening any window.
pub fn logger() -> Logger {
    let (sender, receiver) = mpsc::channel::<Option<String>>();
    let _ = LOG.set(sender.clone());
    let worker = thread::spawn(move || {
        while let Ok(Some(message)) = receiver.recv() {
            eprintln!("{message}");
        }
    });
    Logger {
        sender,
        worker: Some(worker),
    }
}

/// Enqueue a sanitized diagnostic without performing UI-thread I/O.
pub fn log(message: String) {
    if let Some(sender) = LOG.get() {
        let _ = sender.send(Some(message));
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        let _ = self.sender.send(None);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
