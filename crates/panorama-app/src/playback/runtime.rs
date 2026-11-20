//! Whether this install can load libmpv, and what to tell someone whose copy can't.
use panorama_mpv::{Mpv, MpvError, MpvLibrary};
use std::path::{Path, PathBuf};

#[cfg(windows)]
const LIBRARY: &str = "libmpv-2.dll";
#[cfg(target_os = "macos")]
const LIBRARY: &str = "libmpv.2.dylib";
#[cfg(not(any(windows, target_os = "macos")))]
const LIBRARY: &str = "libmpv.so.2";

/// The Windows build of libmpv links the Vulkan loader, which only some graphics drivers install.
#[cfg(windows)]
const RUNTIME_FILES: &[&str] = &[LIBRARY, "vulkan-1.dll"];
#[cfg(not(windows))]
const RUNTIME_FILES: &[&str] = &[LIBRARY];

/// Why playback can't start on this install.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeProblem {
    /// Opened from inside a zip, so Windows extracted the exe alone into a temp folder.
    RunningFromZip,
    /// The library is not where Panorama looks for it.
    Missing,
    /// The library is there but the system would not load it.
    Blocked,
    /// The library loads but is not the client API Panorama was built for.
    WrongVersion,
    Unsupported,
}

impl RuntimeProblem {
    /// The `--runtime-problem` value that forces this state for a capture.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "missing" => Self::Missing,
            "zip" => Self::RunningFromZip,
            "blocked" => Self::Blocked,
            "version" => Self::WrongVersion,
            _ => return None,
        })
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::RunningFromZip => {
                "Panorama is running from inside a zip. Extract the zip first, then open panorama.exe from the extracted folder."
            }
            Self::Missing if cfg!(windows) => {
                "Panorama's playback files aren't next to panorama.exe. Extract the whole zip again, or reinstall Panorama."
            }
            Self::Missing if cfg!(debug_assertions) => {
                "The playback library is missing. Run scripts/stage-macos-libmpv.sh, or set PANORAMA_LIBMPV_DIR to the folder that holds libmpv.2.dylib."
            }
            Self::Missing => {
                "The playback library is missing from Panorama.app. Reinstall Panorama."
            }
            Self::Blocked if cfg!(windows) => {
                "Windows couldn't load libmpv-2.dll. Check Windows Security, Protection history, in case it was blocked, then reinstall Panorama."
            }
            Self::Blocked => "macOS couldn't load the playback library. Reinstall Panorama.",
            Self::WrongVersion if cfg!(windows) => {
                "libmpv-2.dll is a version Panorama can't use. Reinstall Panorama."
            }
            Self::WrongVersion => {
                "The playback library is a version Panorama can't use. Reinstall Panorama."
            }
            Self::Unsupported => "Playback isn't supported on this system yet.",
        }
    }
}

/// What a failed load leaves to go on.
pub struct Install<'a> {
    pub exe: Option<&'a Path>,
    pub temp: &'a Path,
    pub runtime_present: bool,
}

/// Names the problem behind a library load, if it failed.
pub fn runtime_problem(load: Result<(), MpvError>, install: &Install) -> Option<RuntimeProblem> {
    let error = load.err()?;
    Some(match error {
        MpvError::MissingSymbol | MpvError::IncompatibleApi => RuntimeProblem::WrongVersion,
        MpvError::UnsupportedPlatform => RuntimeProblem::Unsupported,
        _ if install.exe.is_some_and(|exe| inside_zip(exe, install.temp)) => {
            RuntimeProblem::RunningFromZip
        }
        _ if install.runtime_present => RuntimeProblem::Blocked,
        _ => RuntimeProblem::Missing,
    })
}

/// Explorer runs an exe opened inside a zip from `%TEMP%\Temp1_<name>.zip\`; 7-Zip and WinRAR
/// use `7zO…` and `Rar$EX…` folders there.
fn inside_zip(exe: &Path, temp: &Path) -> bool {
    let exe = exe.to_string_lossy().to_lowercase().replace('/', "\\");
    let temp = temp.to_string_lossy().to_lowercase().replace('/', "\\");
    let Some(rest) = exe.strip_prefix(temp.trim_end_matches('\\')).or_else(|| {
        exe.split_once("\\appdata\\local\\temp\\")
            .map(|(_, rest)| rest)
    }) else {
        return false;
    };
    rest.split('\\').any(|folder| {
        let numbered = folder
            .strip_prefix("temp")
            .and_then(|tail| tail.split_once('_'))
            .is_some_and(|(digits, _)| {
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            });
        numbered
            || folder.ends_with(".zip")
            || folder.starts_with("7zo")
            || folder.starts_with("rar$ex")
    })
}

/// `PANORAMA_LIBMPV_DIR`, else a debug build's staged copy in the repo, else `None` for the
/// install's own folder.
pub fn library_dir() -> Option<PathBuf> {
    std::env::var_os("PANORAMA_LIBMPV_DIR")
        .map(PathBuf::from)
        .or_else(staged_dir)
}

fn staged_dir() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2)?;
    let dir = if cfg!(windows) {
        repo.join(".cache/panorama/windows-libmpv/current")
    } else {
        repo.join("target/libmpv")
    };
    dir.join(LIBRARY).is_file().then_some(dir)
}

fn runtime_files_present(dir: &Path) -> bool {
    RUNTIME_FILES.iter().all(|file| dir.join(file).is_file())
}

/// Loads libmpv from where playback will, naming the problem when it can't. Blocking.
pub fn load() -> Result<MpvLibrary, RuntimeProblem> {
    let dir = library_dir();
    Mpv::load_library(dir.as_deref()).map_err(|error| {
        let exe = std::env::current_exe().ok();
        let folders = match (&dir, exe.as_deref().and_then(Path::parent)) {
            (Some(dir), _) => vec![dir.clone()],
            (None, Some(exe)) => vec![exe.join("../Frameworks"), exe.to_path_buf()],
            (None, None) => vec![],
        };
        let install = Install {
            exe: exe.as_deref(),
            temp: &std::env::temp_dir(),
            runtime_present: folders.iter().any(|dir| runtime_files_present(dir)),
        };
        runtime_problem(Err(error), &install).unwrap_or(RuntimeProblem::Missing)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMP: &str = r"C:\Users\sam\AppData\Local\Temp";

    fn diagnose(error: MpvError, exe: &str, runtime_present: bool) -> Option<RuntimeProblem> {
        runtime_problem(
            Err(error),
            &Install {
                exe: Some(Path::new(exe)),
                temp: Path::new(TEMP),
                runtime_present,
            },
        )
    }

    #[test]
    fn a_library_that_loads_is_no_problem() {
        let install = Install {
            exe: Some(Path::new(
                r"C:\Users\sam\AppData\Local\Temp\Temp1_panorama.zip\panorama.exe",
            )),
            temp: Path::new(TEMP),
            runtime_present: false,
        };
        assert_eq!(runtime_problem(Ok(()), &install), None);
    }

    #[test]
    fn an_exe_run_from_inside_a_zip_is_told_to_extract() {
        for exe in [
            r"C:\Users\sam\AppData\Local\Temp\Temp1_panorama-windows.zip\panorama.exe",
            r"C:\Users\sam\AppData\Local\Temp\Temp12_panorama.zip\Panorama\panorama.exe",
            r"C:\Users\sam\AppData\Local\Temp\7zO4C21A8B3\panorama.exe",
            r"C:\Users\sam\AppData\Local\Temp\Rar$EXa1234.5678\panorama.exe",
            r"C:\Users\SAMWID~1\AppData\Local\Temp\Temp1_panorama.zip\panorama.exe",
        ] {
            assert_eq!(
                diagnose(MpvError::LibraryUnavailable, exe, false),
                Some(RuntimeProblem::RunningFromZip),
                "{exe}"
            );
        }
    }

    #[test]
    fn a_missing_library_outside_a_zip_is_missing_and_a_present_one_was_blocked() {
        for exe in [
            r"C:\Program Files\Panorama\panorama.exe",
            r"C:\Users\sam\Downloads\panorama.zip\panorama.exe",
            r"C:\Users\sam\AppData\Local\Temp\panorama\panorama.exe",
            "/Applications/Panorama.app/Contents/MacOS/panorama",
        ] {
            assert_eq!(
                diagnose(MpvError::LibraryUnavailable, exe, false),
                Some(RuntimeProblem::Missing),
                "{exe}"
            );
            assert_eq!(
                diagnose(MpvError::LibraryUnavailable, exe, true),
                Some(RuntimeProblem::Blocked),
                "{exe}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_library_without_the_vulkan_loader_beside_it_is_missing_files() {
        let dir = std::env::temp_dir().join(format!("panorama-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LIBRARY), b"").unwrap();
        let without_loader = runtime_files_present(&dir);
        std::fs::write(dir.join("vulkan-1.dll"), b"").unwrap();
        let with_loader = runtime_files_present(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!without_loader);
        assert!(with_loader);
    }

    #[test]
    fn a_library_with_the_wrong_api_is_the_wrong_version() {
        let exe = r"C:\Program Files\Panorama\panorama.exe";
        for error in [MpvError::MissingSymbol, MpvError::IncompatibleApi] {
            assert_eq!(
                diagnose(error, exe, true),
                Some(RuntimeProblem::WrongVersion)
            );
        }
        assert_eq!(
            diagnose(MpvError::UnsupportedPlatform, exe, false),
            Some(RuntimeProblem::Unsupported)
        );
    }
}
