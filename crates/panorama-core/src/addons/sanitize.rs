use stremio_core::types::{
    addon::ResourceResponse,
    resource::{MetaItemPreview, Video},
};
use url::Url;

use super::{Episode, FailureKind, FilmDetails, Kind, StreamSource, TransportResponse};

pub(super) const BODY_LIMIT: usize = 8 * 1024 * 1024;

pub(super) fn valid_id(id: &str) -> bool {
    let length = id.chars().count();
    (1..=200).contains(&length)
        && !id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '?' | '#'))
}

pub(super) fn text(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// One origin country per film: addons send lists ("France, Belgium") and long official names.
pub(super) fn country(value: &str) -> Option<String> {
    let first = value
        .split([',', '/', '&', ';', '|'])
        .map(str::trim)
        .find(|part| !part.is_empty())?;
    let name = match first {
        "United States of America" | "U.S.A." | "U.S." => "United States",
        "United Kingdom of Great Britain and Northern Ireland" | "UK" | "U.K." => "United Kingdom",
        code if matches!(code.len(), 2 | 3) => COUNTRY_CODES
            .iter()
            .find(|(two, three, _)| {
                code.eq_ignore_ascii_case(two) || code.eq_ignore_ascii_case(three)
            })
            .map_or(code, |(_, _, name)| name),
        other => other,
    };
    Some(text(name, 300))
}

/// ISO 3166 alpha-2 and alpha-3 codes, which some addons send ("usa", "KOR"), with the short
/// English name a reader knows them by.
const COUNTRY_CODES: &[(&str, &str, &str)] = &[
    ("AF", "AFG", "Afghanistan"),
    ("AL", "ALB", "Albania"),
    ("DZ", "DZA", "Algeria"),
    ("AD", "AND", "Andorra"),
    ("AO", "AGO", "Angola"),
    ("AG", "ATG", "Antigua and Barbuda"),
    ("AR", "ARG", "Argentina"),
    ("AM", "ARM", "Armenia"),
    ("AU", "AUS", "Australia"),
    ("AT", "AUT", "Austria"),
    ("AZ", "AZE", "Azerbaijan"),
    ("BS", "BHS", "Bahamas"),
    ("BH", "BHR", "Bahrain"),
    ("BD", "BGD", "Bangladesh"),
    ("BB", "BRB", "Barbados"),
    ("BY", "BLR", "Belarus"),
    ("BE", "BEL", "Belgium"),
    ("BZ", "BLZ", "Belize"),
    ("BJ", "BEN", "Benin"),
    ("BT", "BTN", "Bhutan"),
    ("BO", "BOL", "Bolivia"),
    ("BA", "BIH", "Bosnia and Herzegovina"),
    ("BW", "BWA", "Botswana"),
    ("BR", "BRA", "Brazil"),
    ("BN", "BRN", "Brunei"),
    ("BG", "BGR", "Bulgaria"),
    ("BF", "BFA", "Burkina Faso"),
    ("BI", "BDI", "Burundi"),
    ("CV", "CPV", "Cape Verde"),
    ("KH", "KHM", "Cambodia"),
    ("CM", "CMR", "Cameroon"),
    ("CA", "CAN", "Canada"),
    ("CF", "CAF", "Central African Republic"),
    ("TD", "TCD", "Chad"),
    ("CL", "CHL", "Chile"),
    ("CN", "CHN", "China"),
    ("CO", "COL", "Colombia"),
    ("KM", "COM", "Comoros"),
    ("CG", "COG", "Congo"),
    ("CD", "COD", "DR Congo"),
    ("CR", "CRI", "Costa Rica"),
    ("CI", "CIV", "Ivory Coast"),
    ("HR", "HRV", "Croatia"),
    ("CU", "CUB", "Cuba"),
    ("CY", "CYP", "Cyprus"),
    ("CZ", "CZE", "Czechia"),
    ("DK", "DNK", "Denmark"),
    ("DJ", "DJI", "Djibouti"),
    ("DM", "DMA", "Dominica"),
    ("DO", "DOM", "Dominican Republic"),
    ("EC", "ECU", "Ecuador"),
    ("EG", "EGY", "Egypt"),
    ("SV", "SLV", "El Salvador"),
    ("GQ", "GNQ", "Equatorial Guinea"),
    ("ER", "ERI", "Eritrea"),
    ("EE", "EST", "Estonia"),
    ("SZ", "SWZ", "Eswatini"),
    ("ET", "ETH", "Ethiopia"),
    ("FJ", "FJI", "Fiji"),
    ("FI", "FIN", "Finland"),
    ("FR", "FRA", "France"),
    ("GA", "GAB", "Gabon"),
    ("GM", "GMB", "Gambia"),
    ("GE", "GEO", "Georgia"),
    ("DE", "DEU", "Germany"),
    ("GH", "GHA", "Ghana"),
    ("GR", "GRC", "Greece"),
    ("GD", "GRD", "Grenada"),
    ("GT", "GTM", "Guatemala"),
    ("GN", "GIN", "Guinea"),
    ("GW", "GNB", "Guinea-Bissau"),
    ("GY", "GUY", "Guyana"),
    ("HT", "HTI", "Haiti"),
    ("HN", "HND", "Honduras"),
    ("HK", "HKG", "Hong Kong"),
    ("HU", "HUN", "Hungary"),
    ("IS", "ISL", "Iceland"),
    ("IN", "IND", "India"),
    ("ID", "IDN", "Indonesia"),
    ("IR", "IRN", "Iran"),
    ("IQ", "IRQ", "Iraq"),
    ("IE", "IRL", "Ireland"),
    ("IL", "ISR", "Israel"),
    ("IT", "ITA", "Italy"),
    ("JM", "JAM", "Jamaica"),
    ("JP", "JPN", "Japan"),
    ("JO", "JOR", "Jordan"),
    ("KZ", "KAZ", "Kazakhstan"),
    ("KE", "KEN", "Kenya"),
    ("KI", "KIR", "Kiribati"),
    ("KP", "PRK", "North Korea"),
    ("KR", "KOR", "South Korea"),
    ("XK", "XKX", "Kosovo"),
    ("KW", "KWT", "Kuwait"),
    ("KG", "KGZ", "Kyrgyzstan"),
    ("LA", "LAO", "Laos"),
    ("LV", "LVA", "Latvia"),
    ("LB", "LBN", "Lebanon"),
    ("LS", "LSO", "Lesotho"),
    ("LR", "LBR", "Liberia"),
    ("LY", "LBY", "Libya"),
    ("LI", "LIE", "Liechtenstein"),
    ("LT", "LTU", "Lithuania"),
    ("LU", "LUX", "Luxembourg"),
    ("MO", "MAC", "Macao"),
    ("MG", "MDG", "Madagascar"),
    ("MW", "MWI", "Malawi"),
    ("MY", "MYS", "Malaysia"),
    ("MV", "MDV", "Maldives"),
    ("ML", "MLI", "Mali"),
    ("MT", "MLT", "Malta"),
    ("MH", "MHL", "Marshall Islands"),
    ("MR", "MRT", "Mauritania"),
    ("MU", "MUS", "Mauritius"),
    ("MX", "MEX", "Mexico"),
    ("FM", "FSM", "Micronesia"),
    ("MD", "MDA", "Moldova"),
    ("MC", "MCO", "Monaco"),
    ("MN", "MNG", "Mongolia"),
    ("ME", "MNE", "Montenegro"),
    ("MA", "MAR", "Morocco"),
    ("MZ", "MOZ", "Mozambique"),
    ("MM", "MMR", "Myanmar"),
    ("NA", "NAM", "Namibia"),
    ("NR", "NRU", "Nauru"),
    ("NP", "NPL", "Nepal"),
    ("NL", "NLD", "Netherlands"),
    ("NZ", "NZL", "New Zealand"),
    ("NI", "NIC", "Nicaragua"),
    ("NE", "NER", "Niger"),
    ("NG", "NGA", "Nigeria"),
    ("MK", "MKD", "North Macedonia"),
    ("NO", "NOR", "Norway"),
    ("OM", "OMN", "Oman"),
    ("PK", "PAK", "Pakistan"),
    ("PW", "PLW", "Palau"),
    ("PS", "PSE", "Palestine"),
    ("PA", "PAN", "Panama"),
    ("PG", "PNG", "Papua New Guinea"),
    ("PY", "PRY", "Paraguay"),
    ("PE", "PER", "Peru"),
    ("PH", "PHL", "Philippines"),
    ("PL", "POL", "Poland"),
    ("PT", "PRT", "Portugal"),
    ("PR", "PRI", "Puerto Rico"),
    ("QA", "QAT", "Qatar"),
    ("RO", "ROU", "Romania"),
    ("RU", "RUS", "Russia"),
    ("RW", "RWA", "Rwanda"),
    ("KN", "KNA", "Saint Kitts and Nevis"),
    ("LC", "LCA", "Saint Lucia"),
    ("VC", "VCT", "Saint Vincent and the Grenadines"),
    ("WS", "WSM", "Samoa"),
    ("SM", "SMR", "San Marino"),
    ("ST", "STP", "São Tomé and Príncipe"),
    ("SA", "SAU", "Saudi Arabia"),
    ("SN", "SEN", "Senegal"),
    ("RS", "SRB", "Serbia"),
    ("SC", "SYC", "Seychelles"),
    ("SL", "SLE", "Sierra Leone"),
    ("SG", "SGP", "Singapore"),
    ("SK", "SVK", "Slovakia"),
    ("SI", "SVN", "Slovenia"),
    ("SB", "SLB", "Solomon Islands"),
    ("SO", "SOM", "Somalia"),
    ("ZA", "ZAF", "South Africa"),
    ("SS", "SSD", "South Sudan"),
    ("ES", "ESP", "Spain"),
    ("LK", "LKA", "Sri Lanka"),
    ("SD", "SDN", "Sudan"),
    ("SR", "SUR", "Suriname"),
    ("SE", "SWE", "Sweden"),
    ("CH", "CHE", "Switzerland"),
    ("SY", "SYR", "Syria"),
    ("TW", "TWN", "Taiwan"),
    ("TJ", "TJK", "Tajikistan"),
    ("TZ", "TZA", "Tanzania"),
    ("TH", "THA", "Thailand"),
    ("TL", "TLS", "Timor-Leste"),
    ("TG", "TGO", "Togo"),
    ("TO", "TON", "Tonga"),
    ("TT", "TTO", "Trinidad and Tobago"),
    ("TN", "TUN", "Tunisia"),
    ("TR", "TUR", "Turkey"),
    ("TM", "TKM", "Turkmenistan"),
    ("TV", "TUV", "Tuvalu"),
    ("UG", "UGA", "Uganda"),
    ("UA", "UKR", "Ukraine"),
    ("AE", "ARE", "United Arab Emirates"),
    ("GB", "GBR", "United Kingdom"),
    ("US", "USA", "United States"),
    ("UY", "URY", "Uruguay"),
    ("UZ", "UZB", "Uzbekistan"),
    ("VU", "VUT", "Vanuatu"),
    ("VA", "VAT", "Vatican City"),
    ("VE", "VEN", "Venezuela"),
    ("VN", "VNM", "Vietnam"),
    ("YE", "YEM", "Yemen"),
    ("ZM", "ZMB", "Zambia"),
    ("ZW", "ZWE", "Zimbabwe"),
    ("GL", "GRL", "Greenland"),
    ("FO", "FRO", "Faroe Islands"),
];

#[test]
fn a_country_code_reads_as_its_name() {
    for (sent, shown) in [
        ("usa", "United States"),
        ("KOR", "South Korea"),
        ("kr", "South Korea"),
        ("gbr, usa", "United Kingdom"),
        ("United States of America", "United States"),
        ("France", "France"),
        ("Zzz", "Zzz"),
    ] {
        assert_eq!(country(sent).as_deref(), Some(shown), "{sent}");
    }
}

fn optional(value: Option<String>, limit: usize) -> Option<String> {
    value.map(|value| text(&value, limit))
}

fn image(value: Option<Url>) -> Option<Url> {
    value.filter(|url| url.scheme() == "https" && url.as_str().len() <= 4096)
}

#[derive(Default)]
pub(super) struct Credits {
    pub director: Vec<String>,
    pub cast: Vec<String>,
    /// Cast and crew with photos, from a TMDB-backed addon's `app_extras`.
    pub people: Vec<crate::credits::Person>,
    pub previews: Vec<super::PreviewCredits>,
}

pub(super) struct Response {
    pub core: ResourceResponse,
    pub credits: Credits,
}

pub(super) fn credits(value: &serde_json::Value) -> Credits {
    let names = |field: &str| match value.get("meta").and_then(|meta| meta.get(field)) {
        Some(serde_json::Value::String(name)) => vec![text(name, 300)],
        Some(serde_json::Value::Array(names)) => names
            .iter()
            .filter_map(|name| name.as_str())
            .take(100)
            .map(|name| text(name, 300))
            .collect(),
        _ => vec![],
    };
    Credits {
        director: names("director"),
        cast: names("cast"),
        people: value
            .get("meta")
            .and_then(|meta| meta.get("app_extras"))
            .map(crate::credits::people_from_extras)
            .unwrap_or_default(),
        previews: value
            .get("metas")
            .and_then(|v| v.as_array())
            .map(|items| items.iter().take(200).collect::<Vec<_>>())
            .unwrap_or_else(|| value.get("meta").into_iter().collect())
            .into_iter()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?;
                if !valid_id(id) {
                    return None;
                }
                let director = match item.get("director") {
                    Some(serde_json::Value::String(name)) => vec![text(name, 300)],
                    Some(serde_json::Value::Array(names)) => names
                        .iter()
                        .filter_map(|v| v.as_str())
                        .take(100)
                        .map(|v| text(v, 300))
                        .collect(),
                    _ => vec![],
                };
                let country = item
                    .get("country")
                    .or_else(|| item.get("originCountry"))
                    .and_then(|v| v.as_str())
                    .and_then(country);
                // ponytail: field names used by TMDB-backed addons; extend when another appears.
                let original_name = ["originalName", "original_title", "originalTitle"]
                    .into_iter()
                    .find_map(|field| item.get(field)?.as_str())
                    .map(|name| text(name.trim(), 300))
                    .filter(|name| !name.is_empty());
                Some(super::PreviewCredits {
                    id: id.into(),
                    director,
                    country,
                    original_name,
                })
            })
            .collect(),
    }
}

pub(super) fn parse(response: TransportResponse) -> Result<Response, FailureKind> {
    let (core, credits) = match response {
        TransportResponse::Bytes(bytes) if bytes.len() > BODY_LIMIT => Err(FailureKind::TooLarge),
        TransportResponse::Bytes(bytes) => {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| FailureKind::InvalidResponse)?;
            let credits = credits(&value);
            let core = serde_json::from_value(value).map_err(|_| FailureKind::InvalidResponse)?;
            Ok((core, credits))
        }
        TransportResponse::Core(response) => {
            if serde_json::to_vec(&response)
                .map_err(|_| FailureKind::InvalidResponse)?
                .len()
                > BODY_LIMIT
            {
                return Err(FailureKind::TooLarge);
            }
            Ok((response, Credits::default()))
        }
        TransportResponse::WithCredits {
            response,
            director,
            cast,
            people,
            previews,
        } => {
            let mut parsed = parse(TransportResponse::Core(response))?;
            parsed.credits = Credits {
                director: director
                    .iter()
                    .take(100)
                    .map(|name| text(name, 300))
                    .collect(),
                cast: cast.iter().take(100).map(|name| text(name, 300)).collect(),
                people,
                previews: previews
                    .into_iter()
                    .take(200)
                    .filter(|p| valid_id(&p.id))
                    .map(|p| super::PreviewCredits {
                        id: p.id,
                        director: p.director.iter().take(100).map(|v| text(v, 300)).collect(),
                        country: p.country.as_deref().and_then(country),
                        original_name: p.original_name.as_deref().map(|v| text(v, 300)),
                    })
                    .collect(),
            };
            return Ok(parsed);
        }
    }?;
    Ok(Response { core, credits })
}

pub(super) fn film(preview: MetaItemPreview, thin: bool) -> Option<FilmDetails> {
    let name = text(&preview.name, 300);
    let kind = Kind::parse(&preview.r#type)?;
    if !valid_id(&preview.id) || (!thin && name.trim().is_empty()) {
        return None;
    }
    let category = |name: &str| {
        preview
            .links
            .iter()
            .filter(|link| link.category.eq_ignore_ascii_case(name))
            .take(100)
            .map(|link| text(&link.name, 300))
            .collect::<Vec<_>>()
    };
    let genres = category("Genres");
    let director = category("Director");
    let cast = category("Cast");
    let imdb_rating = category("imdb").into_iter().next();
    let links: Vec<_> = preview
        .links
        .into_iter()
        .filter(|link| {
            matches!(link.url.scheme(), "http" | "https") && link.url.as_str().len() <= 4096
        })
        .take(100)
        .map(|mut link| {
            link.name = text(&link.name, 300);
            link.category = text(&link.category, 300);
            link
        })
        .collect();
    Some(FilmDetails {
        id: preview.id,
        kind,
        name,
        released: preview.released.map(|date| date.format("%F").to_string()),
        release_info: optional(
            preview
                .release_info
                .or_else(|| preview.released.map(|date| date.format("%Y").to_string())),
            100,
        ),
        runtime: optional(preview.runtime, 100),
        genres,
        description: optional(preview.description, 5000),
        director,
        origin_country: category_country(&links),
        original_name: None,
        cast,
        people: vec![],
        poster: image(preview.poster),
        background: image(preview.background),
        logo: image(preview.logo),
        imdb_rating,
        links,
        episodes: vec![],
    })
}

/// At most this many episodes per title; long-running anime reach the thousands.
const EPISODE_LIMIT: usize = 5000;

/// Episodes in watch order with specials last. A video without `series_info` is numbered by
/// its position in season 1. Videos with unusable ids are dropped; ids are kept verbatim.
pub(super) fn episodes(videos: &[Video]) -> Vec<Episode> {
    let mut episodes = videos
        .iter()
        .enumerate()
        .filter(|(_, video)| valid_id(&video.id))
        .map(|(position, video)| {
            let (season, number) = match &video.series_info {
                Some(info) => (info.season, info.episode),
                None => (1, u32::try_from(position + 1).unwrap_or(u32::MAX)),
            };
            Episode {
                id: video.id.clone(),
                season,
                number,
                numbered: video.series_info.is_some(),
                title: text(&video.title, 300),
                overview: optional(video.overview.clone(), 5000),
                thumbnail: video
                    .thumbnail
                    .as_deref()
                    .and_then(|value| Url::parse(value).ok())
                    .and_then(|url| image(Some(url))),
                released: video.released.map(|date| date.format("%F").to_string()),
            }
        })
        .collect::<Vec<_>>();
    episodes.sort_by_key(|episode| (episode.season == 0, episode.season, episode.number));
    episodes.truncate(EPISODE_LIMIT);
    episodes
}

pub(super) fn films(response: Response, thin: bool) -> Result<Vec<FilmDetails>, FailureKind> {
    let previews = match response.core {
        ResourceResponse::Metas { metas } => metas,
        ResourceResponse::MetasDetailed { metas_detailed } => metas_detailed
            .into_iter()
            .map(|item| item.preview)
            .collect(),
        _ => return Err(FailureKind::InvalidResponse),
    };
    Ok(previews
        .into_iter()
        .filter_map(|preview| film(preview, thin))
        .map(|mut film| {
            if let Some(credits) = response.credits.previews.iter().find(|p| p.id == film.id) {
                if !credits.director.is_empty() {
                    film.director.clone_from(&credits.director);
                }
                if credits.country.is_some() {
                    film.origin_country.clone_from(&credits.country);
                }
                if credits.original_name.is_some() {
                    film.original_name.clone_from(&credits.original_name);
                }
            }
            film
        })
        .take(200)
        .collect())
}

fn category_country(links: &[stremio_core::types::resource::Link]) -> Option<String> {
    links
        .iter()
        .find(|link| link.category.eq_ignore_ascii_case("country"))
        .and_then(|link| country(&link.name))
}

/// At most this many subtitle files from one addon or one stream.
const SUBTITLE_LIMIT: usize = 100;

fn subtitle(
    item: stremio_core::types::resource::Subtitles,
    addon: &str,
) -> Option<super::SubtitleSource> {
    (matches!(item.url.scheme(), "https" | "http") && item.url.as_str().len() <= 4096).then(|| {
        super::SubtitleSource {
            url: item.url,
            lang: text(item.lang.trim(), 50),
            addon: text(addon, 300),
        }
    })
}

pub(super) fn subtitles(
    response: Response,
    addon: &str,
) -> Result<Vec<super::SubtitleSource>, FailureKind> {
    let ResourceResponse::Subtitles { subtitles } = response.core else {
        return Err(FailureKind::InvalidResponse);
    };
    Ok(subtitles
        .into_iter()
        .filter_map(|item| subtitle(item, addon))
        .take(SUBTITLE_LIMIT)
        .collect())
}

impl StreamSource {
    /// Subtitle files the stream carries itself, credited to `addon`.
    pub fn subtitles(&self, addon: &str) -> Vec<super::SubtitleSource> {
        self.stream
            .subtitles
            .iter()
            .cloned()
            .filter_map(|item| subtitle(item, addon))
            .take(SUBTITLE_LIMIT)
            .collect()
    }
}

pub(super) fn sources(response: Response) -> Result<Vec<StreamSource>, FailureKind> {
    let ResourceResponse::Streams { streams } = response.core else {
        return Err(FailureKind::InvalidResponse);
    };
    Ok(streams
        .into_iter()
        .take(200)
        .map(|mut stream| {
            stream.name = optional(stream.name, 300);
            stream.description = optional(stream.description, 5000);
            stream.thumbnail = stream.thumbnail.filter(|value| {
                Url::parse(value)
                    .ok()
                    .and_then(|url| image(Some(url)))
                    .is_some()
            });
            StreamSource {
                name: stream.name.clone(),
                title: stream.description.clone(),
                description: stream.description.clone(),
                stream,
            }
        })
        .collect())
}
