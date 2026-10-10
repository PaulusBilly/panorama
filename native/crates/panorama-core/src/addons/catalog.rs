use futures::{FutureExt, SinkExt};
use stremio_core::types::addon::{ExtraValue, ResourcePath, ResourceResponse};

use super::{
    AddonClient, CatalogRef, CatalogStream, FailureKind, Installation, Page, ResourceEvent,
    delivery, sanitize,
};
use crate::{store::Key, stremio::Descriptor};

impl AddonClient {
    /// Lists movie catalogs with no required extras, in account order.
    /// An empty installation list exposes only Cinemeta's top movie catalog.
    pub fn catalogs(&self, addons: &[Descriptor]) -> Vec<CatalogRef> {
        self.catalog_plan(addons)
            .into_iter()
            .map(|(_, catalog)| catalog)
            .collect()
    }

    pub(super) fn catalog_plan(&self, addons: &[Descriptor]) -> Vec<(Installation, CatalogRef)> {
        if addons.is_empty() {
            return vec![self.cinemeta_catalog()];
        }
        self.installations(addons)
            .flat_map(|addon| {
                addon
                    .descriptor
                    .manifest
                    .catalogs
                    .iter()
                    .take(100)
                    .filter(|catalog| {
                        catalog.r#type == "movie"
                            && catalog.extra.iter().all(|extra| !extra.is_required)
                            && Key::catalog(addon.key.as_str(), &catalog.id).is_ok()
                    })
                    .map(|catalog| {
                        (
                            addon.clone(),
                            CatalogRef {
                                addon: addon.key.clone(),
                                catalog_id: catalog.id.clone(),
                                name: sanitize::text(
                                    catalog.name.as_deref().unwrap_or(&catalog.id),
                                    300,
                                ),
                            },
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn cinemeta_catalog(&self) -> (Installation, CatalogRef) {
        let addon = self.cinemeta();
        let catalog = CatalogRef {
            addon: addon.key.clone(),
            catalog_id: "top".into(),
            name: "Cinemeta".into(),
        };
        (addon, catalog)
    }

    /// Delivers a cached page, then tries the selected catalog, remaining catalogs,
    /// and Cinemeta. Removed selections use the first current catalog.
    pub fn catalog(&self, addons: &[Descriptor], remembered: Option<CatalogRef>) -> CatalogStream {
        let client = self.for_request();
        let mut plan = self.catalog_plan(addons);
        if let Some(index) = remembered.and_then(|remembered| {
            plan.iter().position(|(_, catalog)| {
                catalog.addon == remembered.addon && catalog.catalog_id == remembered.catalog_id
            })
        }) {
            let selected = plan.remove(index);
            plan.insert(0, selected);
        }
        let fallback = self.cinemeta_catalog();
        if !plan.iter().any(|(_, catalog)| catalog == &fallback.1) {
            plan.push(fallback);
        }
        delivery(&self.runtime, move |mut sender| {
            async move {
                for (_, catalog) in &plan {
                    let Ok(key) = Key::catalog(catalog.addon.as_str(), &catalog.catalog_id) else {
                        continue;
                    };
                    match client.read::<Page>(key).await {
                        Ok(Some(page))
                            if page.catalog.addon == catalog.addon
                                && page.catalog.catalog_id == catalog.catalog_id
                                && !page.items.is_empty() =>
                        {
                            if sender.send(ResourceEvent::CacheHit(page)).await.is_err() {
                                return;
                            }
                            break;
                        }
                        _ => {}
                    }
                }
                let mut failure = FailureKind::InvalidResponse;
                for (addon, catalog) in plan {
                    let path = ResourcePath::without_extra("catalog", "movie", &catalog.catalog_id);
                    let result = client
                        .request(&addon, path, 8)
                        .await
                        .and_then(|response| page(response, &addon, catalog.clone(), 0))
                        .and_then(|page| {
                            if page.items.is_empty() {
                                Err(FailureKind::InvalidResponse)
                            } else {
                                Ok(page)
                            }
                        });
                    match result {
                        Ok(page) => {
                            let result =
                                match Key::catalog(catalog.addon.as_str(), &catalog.catalog_id) {
                                    Ok(key) => client.write(key, &page).await,
                                    Err(_) => Err(FailureKind::Storage),
                                };
                            let _ = sender.send(ResourceEvent::Fresh(page)).await;
                            if let Err(kind) = result {
                                let _ = sender.send(ResourceEvent::Failed(kind)).await;
                            }
                            return;
                        }
                        Err(FailureKind::Offline) => {
                            failure = FailureKind::Offline;
                        }
                        Err(kind) if failure != FailureKind::Offline => failure = kind,
                        Err(_) => {}
                    }
                }
                let _ = sender.send(ResourceEvent::Failed(failure)).await;
            }
            .boxed()
        })
    }

    /// Fetches the next page from the actual Home source without replacing its cache.
    /// Empty results finish paging; dropping the stream cancels the request.
    pub fn catalog_next(
        &self,
        addons: &[Descriptor],
        catalog: CatalogRef,
        skip: usize,
    ) -> CatalogStream {
        let client = self.for_request();
        let installation = self
            .catalog_plan(addons)
            .into_iter()
            .chain(std::iter::once(self.cinemeta_catalog()))
            .find(|(_, source)| source == &catalog)
            .map(|(addon, _)| addon);
        delivery(&self.runtime, move |mut sender| {
            async move {
                let result = match installation {
                    Some(addon) if skip <= 100_000 => {
                        let path = ResourcePath::with_extra(
                            "catalog",
                            "movie",
                            &catalog.catalog_id,
                            &[ExtraValue {
                                name: "skip".into(),
                                value: skip.to_string(),
                            }],
                        );
                        client
                            .request(&addon, path, 8)
                            .await
                            .and_then(|response| page(response, &addon, catalog, skip))
                    }
                    _ => Err(FailureKind::InvalidInput),
                };
                let event = match result {
                    Ok(page) => ResourceEvent::Fresh(page),
                    Err(kind) => ResourceEvent::Failed(kind),
                };
                let _ = sender.send(event).await;
            }
            .boxed()
        })
    }
}

fn page(
    response: sanitize::Response,
    addon: &Installation,
    catalog: CatalogRef,
    skip: usize,
) -> Result<Page, FailureKind> {
    let count = match &response.core {
        ResourceResponse::Metas { metas } => metas.len(),
        ResourceResponse::MetasDetailed { metas_detailed } => metas_detailed.len(),
        _ => return Err(FailureKind::InvalidResponse),
    };
    let paging = addon.descriptor.transport_url == *stremio_core::constants::CINEMETA_URL
        || addon
            .descriptor
            .manifest
            .catalogs
            .iter()
            .find(|c| c.id == catalog.catalog_id && c.r#type == "movie")
            .is_some_and(|c| c.extra.iter().any(|e| e.name == "skip"));
    Ok(Page {
        catalog,
        items: sanitize::films(response, false)?,
        next_skip: skip.saturating_add(count),
        has_more: paging && count > 0,
    })
}
