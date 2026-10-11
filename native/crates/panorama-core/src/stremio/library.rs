use super::*;
use crate::addons::FilmDetails;
use stremio_core::types::resource::MetaItemPreview;

impl CoreSession {
    /// Read whether a film is saved as a permanent, nonremoved library item.
    pub fn is_watchlisted(&self, id: &str) -> bool {
        self.running.as_ref().is_some_and(|running| {
            running
                .runtime
                .model()
                .unwrap_or_else(|p| p.into_inner())
                .ctx
                .inner
                .library
                .items
                .get(id)
                .is_some_and(|item| !item.removed && !item.temp)
        })
    }

    /// Add or remove a film through the pinned core's library actions and drain persistence.
    pub async fn set_watchlisted(
        &mut self,
        film: &FilmDetails,
        saved: bool,
    ) -> Result<(), CoreError> {
        if !self.is_signed_in() || crate::store::Key::meta(&film.id).is_err() {
            return Err(CoreErrorKind::Other.into());
        }
        if self.is_watchlisted(&film.id) == saved {
            return Ok(());
        }
        let preview: MetaItemPreview = serde_json::from_value(serde_json::json!({
            "id": film.id, "type": "movie", "name": film.name, "poster": film.poster,
            "background": film.background, "logo": film.logo, "description": film.description,
            "releaseInfo": film.release_info, "runtime": film.runtime,
        }))
        .map_err(|_| CoreError::from(CoreErrorKind::Other))?;
        let running = self.running.as_ref().ok_or(CoreErrorKind::Other)?;
        let mut changes = self.subscribe();
        running.dispatch(if saved {
            ActionCtx::AddToLibrary(preview)
        } else {
            ActionCtx::RemoveFromLibrary(film.id.clone())
        });
        tokio::time::timeout(TIMEOUT, async {
            while self.is_watchlisted(&film.id) != saved {
                if changes.recv().await.is_err() {
                    return Err(CoreError::from(CoreErrorKind::Other));
                }
            }
            self.state.finish_concurrent().await;
            self.state.drain().await
        })
        .await
        .map_err(|_| CoreError::from(CoreErrorKind::Timeout))?
    }
}
