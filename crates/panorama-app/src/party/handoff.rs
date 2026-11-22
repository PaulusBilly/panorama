//! A second launch carrying an invite link passes it to the window that is already open through
//! one file in the data directory: the store lock keeps a second process from doing anything else.
use std::{
    io,
    path::Path,
    time::{Duration, SystemTime},
};

const FILE: &str = "party-invite";
/// A link left by a launch whose window never picked it up is not acted on later.
const FRESH: Duration = Duration::from_secs(60);
/// How often the open window looks for a handed-off link.
pub const POLL: Duration = Duration::from_secs(2);

/// Leaves `link` for the open window. The file appears whole, so a poll never reads half a link.
pub fn deliver(dir: &Path, link: &str) -> io::Result<()> {
    let staged = dir.join(format!("{FILE}.tmp"));
    write_private(&staged, link)?;
    std::fs::rename(&staged, dir.join(FILE))
}

/// Hands the open window the first valid invite among the links the system opened, as macOS
/// reports them to a running app.
pub fn deliver_first_valid(dir: &Path, urls: &[String]) {
    if let Some(link) = urls
        .iter()
        .find(|url| panorama_core::party::transport::parse_invite(url).is_ok())
    {
        let _ = deliver(dir, link);
    }
}

/// Takes the handed-off link, if one is waiting, fresh and valid. The file is removed either way,
/// so each link is joined at most once.
pub fn take(dir: &Path, now: SystemTime) -> Option<String> {
    let path = dir.join(FILE);
    let modified = std::fs::metadata(&path).ok()?.modified().ok();
    let link = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);
    let age = modified.and_then(|modified| now.duration_since(modified).ok());
    if age.is_some_and(|age| age > FRESH) {
        return None;
    }
    let link = link.ok()?.trim().to_owned();
    panorama_core::party::transport::parse_invite(&link)
        .is_ok()
        .then_some(link)
}

/// Writes a file only its owner can read: it holds a party secret.
pub fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    file.write_all(contents.as_bytes())
}

/// A real loopback invite: it parses, and dials nothing in a test.
#[cfg(test)]
pub(crate) const SAMPLE_INVITE: &str = "panorama://party/endpointabfncsk6wbgqbo24zpi6uvd3doylqboicxza45a5l6yfan47exmbkaibab7qaaabyolag?s=08d5937bec6af09d66465af023eaa41c";

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("panorama-handoff-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_delivered_link_is_taken_once() {
        let dir = dir("once");
        deliver(&dir, SAMPLE_INVITE).unwrap();
        assert_eq!(
            take(&dir, SystemTime::now()).as_deref(),
            Some(SAMPLE_INVITE)
        );
        assert_eq!(take(&dir, SystemTime::now()), None);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_newer_link_replaces_one_not_yet_taken() {
        let dir = dir("newer");
        deliver(&dir, "panorama://party/old?s=00").unwrap();
        deliver(&dir, SAMPLE_INVITE).unwrap();
        assert_eq!(
            take(&dir, SystemTime::now()).as_deref(),
            Some(SAMPLE_INVITE)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stale_link_is_dropped_unjoined() {
        let dir = dir("stale");
        deliver(&dir, SAMPLE_INVITE).unwrap();
        let later = SystemTime::now() + Duration::from_secs(61);
        assert_eq!(take(&dir, later), None);
        deliver(&dir, SAMPLE_INVITE).unwrap();
        assert_eq!(
            take(&dir, SystemTime::now()).as_deref(),
            Some(SAMPLE_INVITE)
        );
        let early = SystemTime::now() + Duration::from_secs(59);
        deliver(&dir, SAMPLE_INVITE).unwrap();
        assert_eq!(take(&dir, early).as_deref(), Some(SAMPLE_INVITE));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn https_sample() -> String {
        let (ticket, secret) = SAMPLE_INVITE
            .strip_prefix("panorama://party/")
            .and_then(|rest| rest.split_once("?s="))
            .unwrap();
        format!(
            "{}#{ticket}.{secret}",
            panorama_core::party::transport::WEB_INVITE
        )
    }

    #[test]
    fn an_https_link_is_accepted() {
        let dir = dir("https");
        let link = https_sample();
        deliver(&dir, &link).unwrap();
        assert_eq!(take(&dir, SystemTime::now()), Some(link.clone()));
        let elsewhere = link.replace("paulusbilly.github.io", "evil.example");
        deliver(&dir, &elsewhere).unwrap();
        assert_eq!(take(&dir, SystemTime::now()), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn opened_urls_hand_over_the_first_valid_invite() {
        let dir = dir("urls");
        let link = https_sample();
        let urls = ["panorama://party/nope?s=zz".to_owned(), link.clone()];
        deliver_first_valid(&dir, &urls);
        assert_eq!(take(&dir, SystemTime::now()), Some(link));
        deliver_first_valid(&dir, &urls[..1]);
        assert_eq!(take(&dir, SystemTime::now()), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_invalid_link_is_dropped_unjoined() {
        let dir = dir("invalid");
        deliver(&dir, "panorama://party/nope?s=zz").unwrap();
        assert_eq!(take(&dir, SystemTime::now()), None);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(take(&dir, SystemTime::now()), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_handed_off_link_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir("private");
        deliver(&dir, SAMPLE_INVITE).unwrap();
        let delivered = std::fs::metadata(dir.join(FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let path = dir.join("invite");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, "invite").unwrap();
        let rewritten = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invite");
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!((delivered, rewritten), (0o600, 0o600));
    }
}
