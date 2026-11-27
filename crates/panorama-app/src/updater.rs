//! Updates from GitHub Releases through Velopack, on Windows installs made by
//! `PanoramaApp-win-Setup.exe`. A zip or dev build has no Velopack install around it, so every
//! call here is a quiet no-op.
use crate::app_state::AppState;
use gpui::{App, Entity};

/// The repository whose releases carry the Velopack feed that `vpk upload github` publishes.
#[cfg(target_os = "windows")]
const REPO: &str = "https://github.com/PaulusBilly/panorama";

/// A newer version, downloaded and waiting to replace this one.
#[derive(Clone)]
pub struct Ready {
    pub version: String,
    /// Absent on a preview, which shows the prompt and applies nothing.
    #[cfg(target_os = "windows")]
    inner: Option<std::sync::Arc<(velopack::UpdateManager, velopack::VelopackAsset)>>,
}

impl Ready {
    /// A made-up ready update, for `--update-ready` captures.
    pub fn preview(version: &str) -> Self {
        Self {
            version: version.into(),
            #[cfg(target_os = "windows")]
            inner: None,
        }
    }
}

/// Runs Velopack's install, update and uninstall hooks. Must come before argument parsing: a hook
/// launch carries Velopack's own flags and exits once its hook has run.
pub fn startup() {
    #[cfg(target_os = "windows")]
    velopack::VelopackApp::build()
        // A second launch from an invite link must never apply an update under the open window;
        // a downloaded update applies when Panorama quits instead.
        .set_auto_apply_on_startup(false)
        .on_before_uninstall_fast_callback(|_| crate::link_handler::unregister())
        .run();
}

/// How often a running Panorama looks again: it is often left open for days, and a release
/// published meanwhile would otherwise wait for the next launch.
#[cfg(target_os = "windows")]
const RECHECK: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Checks for a newer release in the background at launch and every [`RECHECK`] after, and
/// downloads it, then records it on `state`.
pub fn check(state: Entity<AppState>, cx: &mut App) {
    #[cfg(target_os = "windows")]
    cx.spawn(async move |cx| {
        use velopack::{UpdateManager, sources::GithubSource};
        let executor = cx.background_executor().clone();
        // Fails outside a Velopack install, which is the normal case for zips and dev builds.
        let Some(manager) = executor
            .spawn(async {
                UpdateManager::new(GithubSource::new(REPO, None, false), None, None).ok()
            })
            .await
        else {
            return;
        };
        loop {
            let manager = manager.clone();
            if let Some(ready) = executor.spawn(async { find_and_download(manager) }).await {
                let _ = state.update(cx, |state, cx| {
                    state.update = Some(ready);
                    cx.notify();
                });
                return;
            }
            executor.timer(RECHECK).await;
        }
    })
    .detach();
    #[cfg(not(target_os = "windows"))]
    let _ = (state, cx);
}

#[cfg(target_os = "windows")]
fn find_and_download(manager: velopack::UpdateManager) -> Option<Ready> {
    use velopack::{UpdateCheck, UpdateManager};
    let log = |message: String| crate::debug::log(format!("Update: {message}"));
    let ready = |manager: UpdateManager, asset: velopack::VelopackAsset| Ready {
        version: asset.Version.clone(),
        inner: Some(std::sync::Arc::new((manager, asset))),
    };
    // An update downloaded on an earlier run is still on disk: offer it without the network.
    if let Some(asset) = manager.get_update_pending_restart() {
        log(format!("{} already downloaded", asset.Version));
        return Some(ready(manager, asset));
    }
    let info = match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(info)) => *info,
        Ok(_) => return None,
        Err(error) => {
            log(format!("check failed: {error}"));
            return None;
        }
    };
    let version = info.TargetFullRelease.Version.clone();
    if let Err(error) = manager.download_updates(&info, None) {
        log(format!("download of {version} failed: {error}"));
        return None;
    }
    log(format!("{version} downloaded"));
    Some(ready(manager, info.TargetFullRelease))
}

/// Hands `ready` to Velopack's updater, which waits for this process to exit, swaps in the new
/// version and, with `restart`, opens it. Returns whether the updater started; only then should
/// the caller quit for it.
pub fn apply_after_exit(ready: &Ready, restart: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        let Some(inner) = &ready.inner else {
            return false;
        };
        let (manager, asset) = &**inner;
        let silent = !restart;
        match manager.wait_exit_then_apply_updates(asset, silent, restart, Vec::<String>::new()) {
            Ok(()) => true,
            Err(error) => {
                crate::debug::log(format!(
                    "Update: apply of {} failed: {error}",
                    ready.version
                ));
                false
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (ready, restart);
        false
    }
}
