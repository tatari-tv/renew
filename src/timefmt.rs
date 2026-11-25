//! RFC 3339 timestamps as renew has always written them.
//!
//! The cache (`check.yml`) and backup (`meta.yml`) files hold timestamps that chrono
//! serialized: UTC, `Z` suffix, and a fractional part only when nonzero, in groups of
//! three digits (`.123`, `.123456`, `.123456789`). `time`'s own RFC 3339 writer trims
//! trailing zeros instead (`.5`), so these helpers reproduce chrono's shape and keep
//! newly written files byte-identical to what earlier builds wrote.

use serde::{Deserialize, Deserializer, Serializer, de};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

/// Format `at` in UTC with a `Z` suffix, as chrono's serde and `to_rfc3339_opts(.., true)` did.
pub(crate) fn format_rfc3339_z(at: OffsetDateTime) -> String {
    format_utc(at, "Z")
}

/// Format `at` in UTC with a `+00:00` suffix, as chrono's `to_rfc3339()` did.
pub(crate) fn format_rfc3339_offset(at: OffsetDateTime) -> String {
    format_utc(at, "+00:00")
}

fn format_utc(at: OffsetDateTime, suffix: &str) -> String {
    let at = at.to_offset(UtcOffset::UTC);
    let nanos = at.nanosecond();
    // `is_multiple_of` would now compile (MSRV is 1.100); the plain remainder checks stay as written.
    let fraction = match (nanos, nanos % 1_000_000, nanos % 1_000) {
        (0, _, _) => String::new(),
        (_, 0, _) => format!(".{:03}", nanos / 1_000_000),
        (_, _, 0) => format!(".{:06}", nanos / 1_000),
        _ => format!(".{nanos:09}"),
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{}",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        fraction,
        suffix
    )
}

/// `YYYY-MM-DD` in UTC, as `%Y-%m-%d` rendered it.
pub(crate) fn format_date(at: OffsetDateTime) -> String {
    let at = at.to_offset(UtcOffset::UTC);
    format!("{:04}-{:02}-{:02}", at.year(), u8::from(at.month()), at.day())
}

/// Parse an RFC 3339 timestamp (any offset) into UTC.
pub(crate) fn parse_rfc3339(text: &str) -> Result<OffsetDateTime, time::error::Parse> {
    OffsetDateTime::parse(text, &Rfc3339).map(|at| at.to_offset(UtcOffset::UTC))
}

/// `#[serde(with = "crate::timefmt::serde_rfc3339")]` for an `OffsetDateTime`.
pub(crate) mod serde_rfc3339 {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(at: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_rfc3339_z(*at))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<OffsetDateTime, D::Error> {
        let text = String::deserialize(d)?;
        parse_rfc3339(&text).map_err(de::Error::custom)
    }
}

/// `#[serde(with = "crate::timefmt::serde_rfc3339_option")]` for `Option<OffsetDateTime>`.
/// Pair with `#[serde(default)]` so an absent key reads as `None`, as chrono's
/// `Option<DateTime>` did.
pub(crate) mod serde_rfc3339_option {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(at: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
        match at {
            Some(at) => s.serialize_str(&format_rfc3339_z(*at)),
            None => s.serialize_none(),
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<OffsetDateTime>, D::Error> {
        match Option::<String>::deserialize(d)? {
            Some(text) => parse_rfc3339(&text).map(Some).map_err(de::Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests;
