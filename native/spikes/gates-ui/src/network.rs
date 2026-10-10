use crate::logic::{JSON_CAP, Poster, parse_catalog, read_capped};
use gpui::RenderImage;
use std::{collections::HashSet, io::Cursor, path::Path, sync::Arc, time::Duration};

pub fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())
}

pub fn catalog(client: &reqwest::blocking::Client) -> Result<Vec<Poster>, String> {
    let mut posters = Vec::new();
    let mut ids = HashSet::new();
    let mut skip = 0;
    while posters.len() < 500 && skip < 5000 {
        let url = if skip == 0 {
            "https://v3-cinemeta.strem.io/catalog/movie/top.json".into()
        } else {
            format!("https://v3-cinemeta.strem.io/catalog/movie/top/skip={skip}.json")
        };
        let response = client
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        let bytes = read_capped(response, JSON_CAP)?;
        let (count, page) = parse_catalog(&bytes)?;
        if count == 0 {
            break;
        }
        skip += count;
        for poster in page {
            if ids.insert(poster.id.clone()) {
                posters.push(poster);
            }
            if posters.len() == 500 {
                break;
            }
        }
    }
    if posters.len() != 500 {
        return Err(format!(
            "Catalog yielded {} HTTPS posters, need 500",
            posters.len()
        ));
    }
    println!("catalog=500 transport=reqwest-blocking/rustls executor=GPUI-background");
    Ok(posters)
}

pub fn poster(
    client: &reqwest::blocking::Client,
    url: &str,
    scale: f32,
    local: Option<&Path>,
) -> Result<Arc<RenderImage>, String> {
    let bytes = if let Some(path) = local {
        read_capped(
            std::fs::File::open(path).map_err(|e| e.to_string())?,
            8 * 1024 * 1024,
        )?
    } else {
        let response = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        read_capped(response, 8 * 1024 * 1024)?
    };
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let width = (180.0 * scale * 1.03).ceil() as u32;
    let height = (270.0 * scale * 1.03).ceil() as u32;
    let mut rgba = reader
        .decode()
        .map_err(|e| e.to_string())?
        .resize_to_fill(width, height, image::imageops::FilterType::Triangle)
        .into_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}
