// The spike needs runtime libmpv FFI for headless playback measurements.
#![allow(unsafe_code)]

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::PathBuf,
    ptr,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

type Handle = *mut c_void;

#[repr(C)]
struct Event {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct Property {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
}

#[repr(C)]
struct LogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: c_int,
}

pub enum Update {
    Time(f64),
    Duration(f64),
    Seek,
    End { reason: i32, error: i32 },
    Error(String),
    Log(String),
}

pub struct Api {
    _library: libloading::Library,
    create: unsafe extern "C" fn() -> Handle,
    option: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    destroy: unsafe extern "C" fn(Handle),
    command: unsafe extern "C" fn(Handle, u64, *const *const c_char) -> c_int,
    observe: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    wait: unsafe extern "C" fn(Handle, f64) -> *const Event,
    error: unsafe extern "C" fn(c_int) -> *const c_char,
    logs: unsafe extern "C" fn(Handle, *const c_char) -> c_int,
}

impl Api {
    pub fn load() -> Result<Self, String> {
        let directory = std::env::var_os("PANORAMA_LIBMPV_DIR").ok_or(
            "PANORAMA_LIBMPV_DIR is unset; set it to the directory containing libmpv-2.dll",
        )?;
        let dll = PathBuf::from(directory).join("libmpv-2.dll");
        if !dll.is_file() {
            return Err(format!("PANORAMA_LIBMPV_DIR is missing {}", dll.display()));
        }
        unsafe {
            let library: libloading::Library =
                libloading::os::windows::Library::load_with_flags(&dll, 0x00000100 | 0x00001000)
                    .map_err(|error| format!("Cannot load {}: {error}", dll.display()))?
                    .into();
            Ok(Self {
                create: *library.get(b"mpv_create\0").map_err(|e| e.to_string())?,
                option: *library
                    .get(b"mpv_set_option_string\0")
                    .map_err(|e| e.to_string())?,
                initialize: *library
                    .get(b"mpv_initialize\0")
                    .map_err(|e| e.to_string())?,
                destroy: *library
                    .get(b"mpv_terminate_destroy\0")
                    .map_err(|e| e.to_string())?,
                command: *library
                    .get(b"mpv_command_async\0")
                    .map_err(|e| e.to_string())?,
                observe: *library
                    .get(b"mpv_observe_property\0")
                    .map_err(|e| e.to_string())?,
                wait: *library
                    .get(b"mpv_wait_event\0")
                    .map_err(|e| e.to_string())?,
                error: *library
                    .get(b"mpv_error_string\0")
                    .map_err(|e| e.to_string())?,
                logs: *library
                    .get(b"mpv_request_log_messages\0")
                    .map_err(|e| e.to_string())?,
                _library: library,
            })
        }
    }

    fn check(&self, result: c_int, operation: &str) -> Result<(), String> {
        if result < 0 {
            let error = unsafe { CStr::from_ptr((self.error)(result)) }.to_string_lossy();
            Err(format!("{operation}: {error}"))
        } else {
            Ok(())
        }
    }
}

struct Session {
    api: Api,
    handle: Handle,
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.handle) };
        println!("mpv destroyed");
    }
}

impl Session {
    fn new(api: Api, source: &str) -> Result<Self, String> {
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        let session = Self { api, handle };
        session.api.check(
            unsafe { (session.api.logs)(handle, c"warn".as_ptr()) },
            "mpv_request_log_messages",
        )?;
        for (name, value) in [
            ("terminal", "no"),
            ("msg-level", "all=warn"),
            ("keep-open", "yes"),
            ("vo", "null"),
            ("ao", "null"),
            ("hwdec", "no"),
            ("cache", "yes"),
            ("demuxer-max-bytes", "64MiB"),
            ("network-timeout", "90"),
            ("osc", "no"),
            ("input-default-bindings", "no"),
        ] {
            session.option(name, value)?;
        }
        session.api.check(
            unsafe { (session.api.initialize)(handle) },
            "mpv_initialize",
        )?;
        for (id, name, format) in [(1, c"time-pos", 5), (2, c"duration", 5)] {
            session.api.check(
                unsafe { (session.api.observe)(handle, id, name.as_ptr(), format) },
                "mpv_observe_property",
            )?;
        }
        session.command(&["loadfile".into(), source.into()])?;
        Ok(session)
    }

    fn option(&self, name: &str, value: &str) -> Result<(), String> {
        let key = CString::new(name).map_err(|e| e.to_string())?;
        let value = CString::new(value).map_err(|e| e.to_string())?;
        self.api.check(
            unsafe { (self.api.option)(self.handle, key.as_ptr(), value.as_ptr()) },
            name,
        )
    }

    fn command(&self, args: &[String]) -> Result<(), String> {
        let strings = args
            .iter()
            .map(|s| CString::new(s.as_str()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut pointers: Vec<_> = strings.iter().map(|s| s.as_ptr()).collect();
        pointers.push(ptr::null());
        self.api.check(
            unsafe { (self.api.command)(self.handle, 0, pointers.as_ptr()) },
            "mpv_command_async",
        )
    }

    fn event(&self) -> Option<Update> {
        let event = unsafe { &*(self.api.wait)(self.handle, 0.05) };
        if event.error < 0 {
            return Some(Update::Error(
                self.api.check(event.error, "mpv event").unwrap_err(),
            ));
        }
        if event.id == 2 && !event.data.is_null() {
            let log = unsafe { &*event.data.cast::<LogMessage>() };
            return Some(Update::Log(unsafe {
                format!(
                    "mpv [{}] {}: {}",
                    CStr::from_ptr(log.level).to_string_lossy(),
                    CStr::from_ptr(log.prefix).to_string_lossy(),
                    CStr::from_ptr(log.text).to_string_lossy().trim_end()
                )
            }));
        }
        if event.id == 7 && !event.data.is_null() {
            let end = unsafe { &*event.data.cast::<EndFile>() };
            return Some(Update::End {
                reason: end.reason,
                error: end.error,
            });
        }
        if event.id == 20 {
            return Some(Update::Seek);
        }
        if event.id != 22 || event.data.is_null() {
            return None;
        }
        let property = unsafe { &*event.data.cast::<Property>() };
        if property.data.is_null() {
            return None;
        }
        unsafe {
            match (event.userdata, property.format) {
                (1, 5) => Some(Update::Time(*property.data.cast::<f64>())),
                (2, 5) => Some(Update::Duration(*property.data.cast::<f64>())),
                _ => None,
            }
        }
    }
}

pub struct Mpv {
    commands: Sender<Option<Vec<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl Mpv {
    pub fn start(api: Api, source: String) -> Result<(Self, Receiver<Update>), String> {
        let (commands, command_rx) = mpsc::channel::<Option<Vec<String>>>();
        let (updates, update_rx) = mpsc::channel();
        let (ready, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let session = match Session::new(api, &source) {
                Ok(session) => session,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let _ = ready.send(Ok(()));
            loop {
                for command in command_rx.try_iter() {
                    match command {
                        Some(args) => {
                            if let Err(error) = session.command(&args) {
                                let _ = updates.send(Update::Error(error));
                            }
                        }
                        None => return,
                    }
                }
                if let Some(update) = session.event()
                    && updates.send(update).is_err()
                {
                    return;
                }
            }
        });
        let mut mpv = Self {
            commands,
            thread: Some(thread),
        };
        if let Err(error) = ready_rx.recv().map_err(|e| e.to_string())? {
            mpv.stop();
            return Err(error);
        }
        Ok((mpv, update_rx))
    }

    pub fn command(&self, args: &[&str]) {
        if let Err(error) = self
            .commands
            .send(Some(args.iter().map(|s| s.to_string()).collect()))
        {
            eprintln!("mpv command channel: {error}");
        }
    }

    pub fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.commands.send(None);
            if thread.join().is_err() {
                eprintln!("mpv event thread panicked");
            }
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.stop();
    }
}
