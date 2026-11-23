#![allow(clippy::unwrap_used)]

use super::*;
use time::macros::datetime;

#[test]
fn test_format_matches_chrono_digit_groups() {
    let base = datetime!(2024-03-14 01:02:03 UTC);
    let cases = [
        (0, "2024-03-14T01:02:03Z"),
        (500_000_000, "2024-03-14T01:02:03.500Z"),
        (120_000_000, "2024-03-14T01:02:03.120Z"),
        (123_000_000, "2024-03-14T01:02:03.123Z"),
        (123_456_000, "2024-03-14T01:02:03.123456Z"),
        (123_456_789, "2024-03-14T01:02:03.123456789Z"),
        (1_000, "2024-03-14T01:02:03.000001Z"),
        (1, "2024-03-14T01:02:03.000000001Z"),
    ];
    for (nanos, want) in cases {
        let at = base.replace_nanosecond(nanos).unwrap();
        assert_eq!(format_rfc3339(at, true), want);
    }
}

#[test]
fn test_format_non_zulu_uses_plus_zero_offset() {
    let at = datetime!(2024-03-14 01:02:03 UTC);
    assert_eq!(format_rfc3339(at, false), "2024-03-14T01:02:03+00:00");
}

#[test]
fn test_format_converts_to_utc() {
    let at = datetime!(2024-03-14 06:32:03 +05:30);
    assert_eq!(format_rfc3339(at, true), "2024-03-14T01:02:03Z");
}

#[test]
fn test_format_date_is_utc_calendar_date() {
    assert_eq!(format_date(datetime!(2024-03-14 23:30:00 -05:00)), "2024-03-15");
}

#[test]
fn test_parse_accepts_z_zero_offset_and_other_offsets() {
    let want = datetime!(2024-03-14 01:02:03 UTC);
    for s in [
        "2024-03-14T01:02:03Z",
        "2024-03-14T01:02:03+00:00",
        "2024-03-14T06:32:03+05:30",
    ] {
        let got = parse_rfc3339(s).unwrap();
        assert_eq!(got, want, "{s}");
        assert_eq!(got.offset(), UtcOffset::UTC, "{s}");
    }
}

#[test]
fn test_parse_fractional_seconds() {
    assert_eq!(
        parse_rfc3339("2024-03-14T01:02:03.5Z").unwrap(),
        datetime!(2024-03-14 01:02:03.5 UTC)
    );
}

#[test]
fn test_parse_rejects_leap_second() {
    // chrono accepted `:60`; time does not. Documented behavior change.
    assert!(parse_rfc3339("2024-03-14T01:02:60Z").is_err());
}

#[test]
fn test_parse_rejects_missing_offset_and_garbage() {
    for s in ["2024-03-14T01:02:03", "not-a-date", "", "2024-03-14"] {
        assert!(parse_rfc3339(s).is_err(), "{s:?}");
    }
}

#[test]
fn test_roundtrip_through_format_and_parse() {
    for nanos in [0, 1, 500_000_000, 123_456_789] {
        let at = datetime!(2024-03-14 01:02:03 UTC).replace_nanosecond(nanos).unwrap();
        assert_eq!(parse_rfc3339(&format_rfc3339(at, true)).unwrap(), at);
    }
}
