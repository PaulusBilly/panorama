//! Retry-After parsing ported from `desktop/main/media-retry.ts`.

use super::range::MAX_SAFE_INTEGER;
use std::time::UNIX_EPOCH;

/// Returns an absolute Unix millisecond deadline, preserving long server waits.
pub fn retry_after_deadline(value: Option<&str>, now_ms: u64) -> Option<u64> {
    let value = value?.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value
            .parse::<u64>()
            .ok()?
            .checked_mul(1000)?
            .checked_add(now_ms)
            .filter(|deadline| *deadline <= MAX_SAFE_INTEGER);
    }
    if !["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        .iter()
        .any(|day| {
            value
                .get(..3)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(day))
        })
    {
        return None;
    }
    let date = httpdate::parse_http_date(value).ok()?;
    let ms = u64::try_from(date.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
    Some(ms.max(now_ms))
}
