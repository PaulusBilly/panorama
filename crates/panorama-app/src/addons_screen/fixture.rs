//! In-process addons for `--fixtures` captures; no network and no storage.
use super::model::{
    Capability, CatalogGroup, InstalledRow, MetaChoice, PreviewCard, RowPending, catalog_groups,
};
use panorama_core::addons::{AddonKey, CatalogRef};

fn key(id: &str) -> Option<AddonKey> {
    serde_json::from_value(format!("{id}#fixture").into()).ok()
}

struct Spec {
    id: &'static str,
    name: &'static str,
    version: &'static str,
    capabilities: &'static [Capability],
    protected: bool,
    configurable: bool,
    catalogs: &'static [&'static str],
}

const SPECS: [Spec; 3] = [
    Spec {
        id: "com.linvo.cinemeta",
        name: "Cinemeta",
        version: "3.0.13",
        capabilities: &[Capability::Catalogs, Capability::Details],
        protected: true,
        configurable: false,
        catalogs: &["Popular", "Featured"],
    },
    Spec {
        id: "torrentio",
        name: "Torrentio",
        version: "0.0.15",
        capabilities: &[Capability::Streams],
        protected: false,
        configurable: true,
        catalogs: &[],
    },
    Spec {
        id: "aiolists",
        name: "AIOLists",
        version: "1.6.0",
        capabilities: &[Capability::Catalogs],
        protected: false,
        configurable: false,
        catalogs: &["Trending this week", "Hidden gems"],
    },
];

/// The three installed addons, in account order.
pub fn rows() -> Vec<InstalledRow> {
    SPECS
        .iter()
        .filter_map(|spec| {
            Some(InstalledRow {
                key: key(spec.id)?,
                name: spec.name.into(),
                version: spec.version.into(),
                logo: None,
                capabilities: spec.capabilities.to_vec(),
                protected: spec.protected,
                configurable: spec.configurable,
                pending: RowPending::None,
            })
        })
        .collect()
}

/// Addons that can supply film details.
pub fn meta_choices() -> Vec<MetaChoice> {
    rows()
        .into_iter()
        .filter(|row| row.capabilities.contains(&Capability::Details))
        .map(|row| MetaChoice {
            key: row.key,
            name: row.name,
        })
        .collect()
}

/// The two-sentence preview any `manifest.json` link produces.
pub fn preview_card() -> PreviewCard {
    PreviewCard {
        name: "Example Addon".into(),
        version: "1.0.0".into(),
        description: Some(
            "Adds a hand-picked catalog of films and a list of places to watch them. \
             Everything it shows is kept in order on this screen."
                .into(),
        ),
        logo: None,
        capabilities: vec![Capability::Catalogs, Capability::Streams],
        types: vec!["Movies"],
    }
}

/// Catalogs of every fixture addon, in account order.
pub fn catalogs() -> Vec<CatalogRef> {
    SPECS
        .iter()
        .flat_map(|spec| {
            let addon = key(spec.id);
            spec.catalogs.iter().filter_map(move |name| {
                Some(CatalogRef {
                    addon: addon.clone()?,
                    catalog_id: name.to_lowercase().replace(' ', "-"),
                    name: (*name).into(),
                })
            })
        })
        .collect()
}

/// Home switcher groups for the fixture account.
pub fn groups() -> Vec<CatalogGroup> {
    let addons: Vec<_> = rows().into_iter().map(|row| (row.key, row.name)).collect();
    catalog_groups(&addons, &catalogs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_belong_to_fixture_addons_and_group_in_account_order() {
        let rows = rows();
        let groups = groups();
        let names: Vec<_> = groups.iter().map(|group| group.addon.as_str()).collect();
        assert_eq!(names, ["Cinemeta", "AIOLists"]);
        let keys: Vec<_> = rows.iter().map(|row| &row.key).collect();
        assert!(
            catalogs()
                .iter()
                .all(|catalog| keys.contains(&&catalog.addon))
        );
        assert!(
            meta_choices()
                .iter()
                .all(|choice| keys.contains(&&choice.key))
        );
    }
}
