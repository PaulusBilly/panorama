use gpui::{App, FontWeight, font};
use std::borrow::Cow;

pub const STATIC: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/DMSans-Regular.ttf"),
    include_bytes!("../assets/fonts/DMSans-Medium.ttf"),
    include_bytes!("../assets/fonts/DMSans-Bold.ttf"),
];
pub const VARIABLE: &[u8] = include_bytes!("../assets/fonts/DMSans[opsz,wght].ttf");
pub const STATIC_FAMILY: &str = "DM Sans 14pt";
pub const VARIABLE_FAMILY: &str = "DM Vari 14pt";

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

pub fn variable_alias() -> &'static [u8] {
    let mut bytes = VARIABLE.to_vec();
    let tables = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let mut head = 0;
    for i in 0..tables {
        let record = 12 + i * 16;
        let offset =
            u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[record + 12..record + 16].try_into().unwrap()) as usize;
        if &bytes[record..record + 4] == b"head" {
            head = offset;
        }
        if &bytes[record..record + 4] != b"name" {
            continue;
        }
        for (old, new) in [("DM Sans", "DM Vari"), ("DMSans", "DMVari")] {
            for (old, new) in [
                (old.as_bytes().to_vec(), new.as_bytes().to_vec()),
                (
                    old.encode_utf16().flat_map(u16::to_be_bytes).collect(),
                    new.encode_utf16().flat_map(u16::to_be_bytes).collect(),
                ),
            ] {
                for start in offset..offset + len - old.len() + 1 {
                    if bytes[start..start + old.len()] == old {
                        bytes[start..start + new.len()].copy_from_slice(&new);
                    }
                }
            }
        }
        let sum = checksum(&bytes[offset..offset + len]);
        bytes[record + 4..record + 8].copy_from_slice(&sum.to_be_bytes());
    }
    bytes[head + 8..head + 12].fill(0);
    let adjustment = 0xb1b0afbau32.wrapping_sub(checksum(&bytes));
    bytes[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    Box::leak(bytes.into_boxed_slice())
}

pub fn register(cx: &mut App) -> Result<String, String> {
    let alias = variable_alias();
    cx.text_system()
        .add_fonts(
            STATIC
                .into_iter()
                .chain([alias])
                .map(Cow::Borrowed)
                .collect(),
        )
        .map_err(|e| e.to_string())?;
    println!(
        "Registered DM families: {:?}",
        cx.text_system()
            .all_font_names()
            .into_iter()
            .filter(|name| name.starts_with("DM"))
            .collect::<Vec<_>>()
    );
    let result = crate::win32::font_probe(&STATIC, alias)?;
    for family in [STATIC_FAMILY, VARIABLE_FAMILY, "Segoe UI"] {
        for weight in [400.0, 500.0, 700.0] {
            let mut requested = font(family);
            requested.weight = FontWeight(weight);
            let id = cx.text_system().resolve_font(&requested);
            let resolved = cx
                .text_system()
                .get_font_for_id(id)
                .ok_or("FontId has no family mapping")?;
            println!(
                "GPUI requested={family} weight={weight} resolved-family={} id={id:?} M-advance-52px={:.3}",
                resolved.family,
                f32::from(
                    cx.text_system()
                        .advance(id, gpui::px(52.0), 'M')
                        .map_err(|e| e.to_string())?
                        .width
                )
            );
            if resolved.family != family {
                return Err(format!(
                    "{family} silently fell back to {}",
                    resolved.family
                ));
            }
        }
    }
    Ok(result)
}
