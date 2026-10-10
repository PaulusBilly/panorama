use std::{
    ffi::{CString, c_char, c_int, c_void},
    path::Path,
    ptr,
    sync::{Arc, Mutex, Weak},
};

use crate::{MpvError, PlayerOptions, PropertyValue, RawWindow, error::check, options::OPTIONS};
mod nodes;
mod receive;

type Handle = *mut c_void;
type Command = unsafe extern "C" fn(Handle, u64, *const *const c_char) -> c_int;
type Set = unsafe extern "C" fn(Handle, u64, *const c_char, c_int, *mut c_void) -> c_int;

pub(crate) struct Api {
    _library: libloading::Library,
    create: unsafe extern "C" fn() -> Handle,
    option: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    terminate: unsafe extern "C" fn(Handle),
    command: Command,
    set: Set,
    observe: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    wait: unsafe extern "C" fn(Handle, f64) -> *const receive::Event,
    wakeup: unsafe extern "C" fn(Handle),
}

impl Api {
    #[cfg(windows)]
    pub(crate) fn load(dir: Option<&Path>) -> Result<Arc<Self>, MpvError> {
        let default = if dir.is_none() {
            Some(std::env::current_exe().map_err(|_| MpvError::LibraryUnavailable)?)
        } else {
            None
        };
        let directory = dir
            .or_else(|| default.as_deref().and_then(Path::parent))
            .ok_or(MpvError::LibraryUnavailable)?;
        let dll = directory
            .join("libmpv-2.dll")
            .canonicalize()
            .map_err(|_| MpvError::LibraryUnavailable)?;
        // SAFETY: Full canonical DLL path; dependencies use only DLL/default directories.
        let library: libloading::Library =
            unsafe { libloading::os::windows::Library::load_with_flags(&dll, 0x100 | 0x1000) }
                .map_err(|_| MpvError::LibraryUnavailable)?
                .into();
        // SAFETY: All symbols use the client.h C ABI; the library outlives every pointer.
        unsafe {
            let version: libloading::Symbol<unsafe extern "C" fn() -> std::os::raw::c_ulong> =
                library
                    .get(b"mpv_client_api_version\0")
                    .map_err(|_| MpvError::MissingSymbol)?;
            if version() >> 16 != 2 {
                return Err(MpvError::IncompatibleApi);
            }
            Ok(Arc::new(Self {
                create: *library
                    .get(b"mpv_create\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                option: *library
                    .get(b"mpv_set_option_string\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                initialize: *library
                    .get(b"mpv_initialize\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                terminate: *library
                    .get(b"mpv_terminate_destroy\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                command: *library
                    .get(b"mpv_command_async\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                set: *library
                    .get(b"mpv_set_property_async\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                observe: *library
                    .get(b"mpv_observe_property\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                wait: *library
                    .get(b"mpv_wait_event\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                wakeup: *library
                    .get(b"mpv_wakeup\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                _library: library,
            }))
        }
    }

    #[cfg(not(windows))]
    pub(crate) fn load(_: Option<&Path>) -> Result<Arc<Self>, MpvError> {
        Err(MpvError::UnsupportedPlatform)
    }

    pub(crate) fn create(
        self: &Arc<Self>,
        surface: Option<RawWindow>,
    ) -> Result<Arc<Client>, MpvError> {
        // SAFETY: Resolved C function, invoked on the dedicated worker.
        let handle = unsafe { (self.create)() };
        if handle.is_null() {
            return Err(MpvError::CreateFailed);
        }
        Ok(Arc::new(Client {
            api: self.clone(),
            handle: handle as usize,
            _surface: surface,
        }))
    }
}

pub(crate) struct Client {
    api: Arc<Api>,
    handle: usize,
    _surface: Option<RawWindow>,
}

impl Client {
    pub(crate) fn initialize(&self, options: &PlayerOptions) -> Result<(), MpvError> {
        for &(key, value, best_effort) in OPTIONS {
            let result = self.option(key, value);
            if !best_effort {
                result?;
            }
        }
        if let Some(window) = &options.wid {
            self.option("wid", &window.id().to_string())?;
        }
        for (key, value) in &options.extra {
            self.option(key, value)?;
        }
        // SAFETY: Live exclusively initialized handle with owned option strings copied by mpv.
        check(unsafe { (self.api.initialize)(self.handle as Handle) })?;
        for (index, name) in receive::OBSERVED.iter().enumerate() {
            let name = cstring(name)?;
            // SAFETY: mpv copies the name; format NODE provides typed, owned event copying.
            check(unsafe {
                (self.api.observe)(self.handle as Handle, index as u64 + 1, name.as_ptr(), 6)
            })?;
        }
        Ok(())
    }

    fn option(&self, name: &str, value: &str) -> Result<(), MpvError> {
        let name = cstring(name)?;
        let value = cstring(value)?;
        // SAFETY: Both NUL-terminated strings live through the call; mpv copies options.
        check(unsafe { (self.api.option)(self.handle as Handle, name.as_ptr(), value.as_ptr()) })
    }

    pub(crate) fn command(&self, args: &[String]) -> Result<(), MpvError> {
        let args = args
            .iter()
            .map(|v| cstring(v))
            .collect::<Result<Vec<_>, _>>()?;
        let mut pointers: Vec<_> = args.iter().map(|s| s.as_ptr()).collect();
        pointers.push(ptr::null());
        // SAFETY: A null-terminated argv of live C strings; async API copies them before return.
        check(unsafe { (self.api.command)(self.handle as Handle, 0, pointers.as_ptr()) })
    }

    pub(crate) fn set(&self, name: &str, value: &mut PropertyValue) -> Result<(), MpvError> {
        let name = cstring(name)?;
        let mut flag;
        let text;
        let mut text_pointer;
        let (format, data) = match value {
            PropertyValue::Bool(v) => {
                flag = i32::from(*v);
                (3, (&mut flag as *mut i32).cast())
            }
            PropertyValue::Integer(v) => (4, (v as *mut i64).cast()),
            PropertyValue::Double(v) => (5, (v as *mut f64).cast()),
            PropertyValue::String(v) => {
                text = cstring(v)?;
                text_pointer = text.as_ptr();
                (1, (&mut text_pointer as *mut *const c_char).cast())
            }
        };
        // SAFETY: The format matches the live scalar/pointer; async API copies data before return.
        check(unsafe { (self.api.set)(self.handle as Handle, 0, name.as_ptr(), format, data) })
    }

    fn wakeup(&self) {
        // SAFETY: Thread-safe non-blocking API on a retained live handle.
        unsafe { (self.api.wakeup)(self.handle as Handle) };
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: Final handle owner, on the teardown worker after event-thread join.
        // The library and video-surface lease remain alive throughout destruction.
        unsafe { (self.api.terminate)(self.handle as Handle) };
    }
}

#[derive(Default)]
pub(crate) struct Wake(Mutex<Weak<Client>>);

impl Wake {
    pub(crate) fn register(&self, client: &Arc<Client>) {
        if let Ok(mut value) = self.0.lock() {
            *value = Arc::downgrade(client);
        }
    }

    pub(crate) fn signal(&self) {
        if let Ok(value) = self.0.try_lock()
            && let Some(client) = value.upgrade()
        {
            client.wakeup();
        }
    }

    pub(crate) fn disable(&self) {
        if let Ok(mut value) = self.0.lock() {
            *value = Weak::new();
        }
    }
}

fn cstring(value: &str) -> Result<CString, MpvError> {
    CString::new(value).map_err(|_| MpvError::InvalidArgument)
}
