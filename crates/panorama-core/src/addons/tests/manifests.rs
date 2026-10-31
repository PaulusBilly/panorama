use super::*;

#[tokio::test(start_paused = true)]
async fn catalog_plans_share_large_manifests() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let catalogs = (0..8)
        .map(|index| json!({"id":format!("catalog{index}"),"type":"movie"}))
        .collect::<Vec<_>>();
    let mut core = addon(
        "large.test",
        "large",
        json!(catalogs),
        json!([]),
        Value::Null,
    )
    .as_core()
    .clone();
    core.manifest.description = Some("x".repeat(7 * 1024 * 1024));
    let addon = Descriptor::from_core(core);
    let plan = client.catalog_plan(std::slice::from_ref(&addon));
    assert_eq!(plan.len(), 8);
    let description = addon
        .as_core()
        .manifest
        .description
        .as_ref()
        .unwrap()
        .as_ptr();
    assert!(plan.iter().all(|(installation, _)| {
        installation
            .descriptor
            .manifest
            .description
            .as_ref()
            .unwrap()
            .as_ptr()
            == description
    }));
}

#[tokio::test(start_paused = true)]
async fn large_manifests_bound_catalog_work_and_copied_strings() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let catalogs = (0..1000)
        .map(|index| {
            json!({"id":format!("catalog{index}"),"type":"movie",
            "name":"n".repeat(1000),"extra":[{"name":"search"}]})
        })
        .collect::<Vec<_>>();
    let mut core = addon(
        "large.test",
        "large",
        json!(catalogs),
        json!([]),
        Value::Null,
    )
    .as_core()
    .clone();
    core.manifest.description = Some("x".repeat(7 * 1024 * 1024));
    let addons = [Descriptor::from_core(core)];
    let plan = client.catalog_plan(&addons);
    assert_eq!(plan.len(), 100);
    assert_eq!(Arc::strong_count(&plan[0].0.descriptor), 101);
    assert!(plan.iter().all(|(_, catalog)| catalog.name.len() == 300));
    drop(plan);
    client.search(&addons, "film").collect::<Vec<_>>().await;
    assert_eq!(fake.hosts().len(), 4);
}

#[tokio::test(start_paused = true)]
async fn search_bounds_catalog_scan_and_catalog_ids() {
    let fake = Fake::default();
    let client = client(&fake).await;
    let mut catalogs = (0..100)
        .map(|index| json!({"id":format!("skip{index}"),"type":"series"}))
        .collect::<Vec<_>>();
    catalogs.push(json!({"id":"late","type":"movie","extra":[{"name":"search"}]}));
    let oversized = json!([{"id":"x".repeat(513),"type":"movie","extra":[{"name":"search"}]}]);
    for catalogs in [json!(catalogs), oversized] {
        let addon = addon("bounds.test", "bounds", catalogs, json!([]), Value::Null);
        assert_eq!(
            client.search(&[addon], "film").collect::<Vec<_>>().await,
            [ResourceEvent::Fresh(vec![])]
        );
    }
    assert!(fake.hosts().is_empty());
}
