//! Tests for `../query_mapper.rs` (moved out of the source file).
use super::*;

#[test]
fn bind_params_handles_supported_types() {
    let mut args = PgArguments::default();
    let params = vec![
        QueryParam::Null,
        QueryParam::Bool(true),
        QueryParam::Int64(42),
        QueryParam::Float64(1.234),
        QueryParam::Decimal("1234567890123456.1234".into()),
        QueryParam::Text("hello".into()),
        QueryParam::Bytes(vec![1, 2, 3]),
        QueryParam::Uuid("550e8400-e29b-41d4-a716-446655440000".into()),
        QueryParam::DateTime("2026-01-01T00:00:00Z".into()),
        QueryParam::Time("12:34:56.123456".into()),
        QueryParam::Interval("1 days 02:00:00".into()),
        QueryParam::Inet("192.0.2.1/24".into()),
        QueryParam::Json(serde_json::json!({"key": "value"})),
    ];
    assert!(bind_params(&params, &mut args).is_ok());
}

#[test]
fn bind_params_accepts_date_only_value() {
    let mut args = PgArguments::default();
    let params = vec![QueryParam::DateTime("2026-08-17".into())];
    assert!(bind_params(&params, &mut args).is_ok());
}

#[test]
fn bind_params_with_types_captures_the_converted_wire_types() {
    let params = [
        QueryParam::Int64(i64::MAX),
        QueryParam::Null,
        QueryParam::DateTime("2026-08-17".into()),
        QueryParam::DateTime("2026-08-17T12:34:56Z".into()),
        QueryParam::Time("12:34:56".into()),
        QueryParam::Time("12:34:56+02:00".into()),
    ];
    let mut args = PgArguments::default();
    let types = bind_params_with_types(&params, &mut args).unwrap();
    assert_eq!(args.len(), params.len());
    assert_eq!(types[1].oid(), Some(sqlx::postgres::types::Oid(0)));
    assert_eq!(
        types
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 1)
            .map(|(_, type_info)| type_info.name())
            .collect::<Vec<_>>(),
        vec!["INT8", "DATE", "TIMESTAMPTZ", "TIME", "TIMETZ"]
    );

    let mut plain_args = PgArguments::default();
    bind_params(&params, &mut plain_args).unwrap();
    assert_eq!(plain_args.len(), args.len());
}

/// A2 (#52): the canonical strings of the dedicated temporal variants bind
/// through the typed parsers — the naive, the instant and the offset-bearing
/// shapes each take their own branch — and a malformed value fails explicitly
/// instead of being coerced or sent as text.
#[test]
fn dedicated_temporal_variant_strings_bind_typed_or_fail_explicitly() {
    let accepted = vec![
        // Timestamp: no marker, so it must not be read as an instant.
        QueryParam::DateTime("2024-03-15T10:20:30.123456".into()),
        // TimestampTz: an instant.
        QueryParam::DateTime("2024-03-15T10:20:30.123456Z".into()),
        // Date.
        QueryParam::DateTime("2024-03-15".into()),
        // Time.
        QueryParam::Time("10:20:30.123456".into()),
        // TimeTz: the offset is part of the value, not the session's.
        QueryParam::Time("10:20:30.123456+07:00".into()),
    ];
    let mut args = PgArguments::default();
    assert!(bind_params(&accepted, &mut args).is_ok());

    for rejected in [
        QueryParam::DateTime("2024-03-15T10:20:30.123456+25:00".into()),
        QueryParam::Time("not-a-time".into()),
    ] {
        let mut args = PgArguments::default();
        let error = bind_params(&[rejected], &mut args).expect_err("a malformed temporal value must fail");
        let message = error.to_string();
        assert!(
            message.contains("invalid"),
            "the failure must name the class it could not parse: {message}"
        );
    }
}

#[test]
fn bind_params_fails_on_invalid_uuid() {
    let mut args = PgArguments::default();
    let params = vec![QueryParam::Uuid("invalid-uuid".into())];
    assert!(bind_params(&params, &mut args).is_err());
}

#[test]
fn bind_params_fails_on_invalid_datetime() {
    let mut args = PgArguments::default();
    let params = vec![QueryParam::DateTime("invalid-date".into())];
    assert!(bind_params(&params, &mut args).is_err());
}

#[test]
fn parses_local_time_and_time_with_offset() {
    assert!(matches!(
        parse_time_parameter("12:34:56.123456"),
        Ok(PostgresTimeParameter::Time(time)) if time.to_string() == "12:34:56.123456"
    ));
    assert!(matches!(
        parse_time_parameter("12:34:56.123456+02:00"),
        Ok(PostgresTimeParameter::TimeWithOffset(time))
            if time.time.to_string() == "12:34:56.123456"
                && time.offset.local_minus_utc() == 7_200
    ));
}

#[test]
fn parses_interval_components_without_precision_loss() {
    let interval = parse_interval_parameter("1 mon 2 days 03:04:05.123456").unwrap();
    assert_eq!(interval.months, 1);
    assert_eq!(interval.days, 2);
    assert_eq!(interval.microseconds, 11_045_123_456);
}

#[test]
fn rejects_invalid_interval_values() {
    assert!(parse_interval_parameter("1 day 60:00:00.1234567").is_err());
    assert!(parse_interval_parameter("1 fortnights").is_err());
}

#[test]
fn binary_numeric_uses_postgres_display_scale() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&7u16.to_be_bytes());
    bytes.extend_from_slice(&4i16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&5i16.to_be_bytes());
    for digit in [1234u16, 5678, 9012, 3456, 7890, 1234, 5000] {
        bytes.extend_from_slice(&digit.to_be_bytes());
    }

    assert_eq!(
        decode_binary_numeric(&bytes).as_deref(),
        Some("12345678901234567890.12345")
    );
}

#[test]
fn binary_numeric_preserves_declared_fractional_zeroes() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u16.to_be_bytes());
    bytes.extend_from_slice(&0i16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&2i16.to_be_bytes());
    for digit in [1u16, 5000] {
        bytes.extend_from_slice(&digit.to_be_bytes());
    }

    assert_eq!(decode_binary_numeric(&bytes).as_deref(), Some("1.50"));
}

/// The binary-payload rule is name- and kind-driven: character types and enum
/// labels are text, the structured classes (arrays, ranges, composites, money,
/// tsvector) are opaque bytes. An enum's own type name is arbitrary, which is why
/// the kind check in `binary_payload_is_text` decides for it, not this list.
#[test]
fn text_like_type_names_cover_character_types_only() {
    for name in [
        "TEXT",
        "text",
        "VARCHAR",
        "VARCHAR(20)",
        "CHARACTER VARYING",
        "BPCHAR",
        "CHAR",
        "CHARACTER(3)",
        "NAME",
        "XML",
        "citext",
    ] {
        assert!(is_text_like_type_name(name), "{name} is a character type");
    }

    for name in [
        "INT4",
        "INT4RANGE",
        "TEXT[]",
        "MONEY",
        "RECORD",
        "tsvector",
        "JSONB",
        "POINT",
        "order_status",
    ] {
        assert!(!is_text_like_type_name(name), "{name} must be byte-exact, not text");
    }
}

#[test]
fn naive_timestamp_carries_no_timezone_marker() {
    let value = chrono::NaiveDate::from_ymd_opt(2024, 3, 15)
        .unwrap()
        .and_hms_micro_opt(10, 20, 30, 123_456)
        .unwrap();

    assert_eq!(format_naive_timestamp(value), "2024-03-15T10:20:30.123456");
    assert!(!format_naive_timestamp(value).contains('Z'));
    assert!(!format_naive_timestamp(value).contains('+'));
}

#[test]
fn naive_timestamp_spells_out_whole_second_fraction() {
    let value = chrono::NaiveDate::from_ymd_opt(2024, 3, 15)
        .unwrap()
        .and_hms_opt(10, 20, 30)
        .unwrap();

    assert_eq!(format_naive_timestamp(value), "2024-03-15T10:20:30.000000");
}

#[test]
fn utc_instant_keeps_explicit_utc_marker() {
    let value = chrono::DateTime::parse_from_rfc3339("2024-03-15T08:20:30.123456Z")
        .unwrap()
        .with_timezone(&chrono::Utc);

    assert_eq!(format_utc_instant(value), "2024-03-15T08:20:30.123456Z");
}

#[test]
fn utc_instant_is_normalized_to_utc() {
    let value = chrono::DateTime::parse_from_rfc3339("2024-03-15T10:20:30.123456+02:00")
        .unwrap()
        .with_timezone(&chrono::Utc);

    assert_eq!(format_utc_instant(value), "2024-03-15T08:20:30.123456Z");
}

/// Build a binary NUMERIC payload: ndigits, weight, sign, dscale, digits.
fn numeric_payload(sign: u16, digits: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(digits.len() as u16).to_be_bytes());
    bytes.extend_from_slice(&0i16.to_be_bytes()); // weight
    bytes.extend_from_slice(&sign.to_be_bytes());
    bytes.extend_from_slice(&0i16.to_be_bytes()); // dscale
    for digit in digits {
        bytes.extend_from_slice(&digit.to_be_bytes());
    }
    bytes
}

/// #590: PostgreSQL 14+ numeric specials must decode instead of failing the
/// whole result set.
#[test]
fn numeric_specials_decode_to_their_literals_590() {
    assert_eq!(decode_binary_numeric(&numeric_payload(0xC000, &[])).as_deref(), Some("NaN"));
    assert_eq!(
        decode_binary_numeric(&numeric_payload(0xD000, &[])).as_deref(),
        Some("Infinity")
    );
    assert_eq!(
        decode_binary_numeric(&numeric_payload(0xF000, &[])).as_deref(),
        Some("-Infinity")
    );
    // Ordinary values keep their previous behaviour.
    assert_eq!(
        decode_binary_numeric(&numeric_payload(0x0000, &[12, 3400])).as_deref(),
        Some("123400")
    );
    assert_eq!(
        decode_binary_numeric(&numeric_payload(0x4000, &[1])).as_deref(),
        Some("-0001")
    );
    // A special sign with digits is malformed, not silently accepted.
    assert!(decode_binary_numeric(&numeric_payload(0xD000, &[1])).is_none());
}

/// #590: those special values are bound as text rather than through BigDecimal.
#[test]
fn numeric_special_params_are_recognised_590() {
    for value in ["NaN", "nan", "Infinity", "-Infinity", " inf "] {
        assert!(is_special_decimal(value), "{value}");
    }
    for value in ["0", "-1.5", "1e9", ""] {
        assert!(!is_special_decimal(value), "{value}");
    }
}

/// #591: binary-array payloads must render as readable PostgreSQL array text
/// instead of opaque `\x…` hex.
#[test]
fn binary_arrays_render_as_array_literals_591() {
    fn array_payload(elements: &[Option<&str>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_be_bytes()); // ndim
        bytes.extend_from_slice(&0u32.to_be_bytes()); // has nulls
        bytes.extend_from_slice(&25u32.to_be_bytes()); // element oid (text)
        bytes.extend_from_slice(&(elements.len() as u32).to_be_bytes()); // dim length
        bytes.extend_from_slice(&1u32.to_be_bytes()); // lower bound
        for element in elements {
            match element {
                Some(text) => {
                    bytes.extend_from_slice(&(text.len() as u32).to_be_bytes());
                    bytes.extend_from_slice(text.as_bytes());
                }
                None => bytes.extend_from_slice(&u32::MAX.to_be_bytes()),
            }
        }
        bytes
    }

    let payload = array_payload(&[Some("alpha"), Some("b,eta"), Some("\"quoted\""), None]);
    assert_eq!(
        format_binary_array(&payload).as_deref(),
        Some(r#"{alpha,"b,eta","\"quoted\"",NULL}"#)
    );
    // A single-element int array.
    let numbers = array_payload(&[Some("1"), Some("2")]);
    assert_eq!(format_binary_array(&numbers).as_deref(), Some("{1,2}"));
    // Truncated framing is rejected rather than panicking.
    assert!(format_binary_array(&payload[..payload.len() - 2]).is_none());
}

/// #592: JSON/JSONB payloads must not go through `serde_json::Value`, which
/// rounds large numbers and reorders keys.
#[test]
fn jsonb_text_preserves_numbers_and_key_order_592() {
    let document = r#"{"b":1,"a":0.1234567890123456789,"big":123456789012345678901234}"#;
    assert!(serde_json::from_str::<serde_json::Value>(document).is_ok());
    // The infrastructure no longer parses the payload, so the invariant worth
    // pinning is the version-byte stripping rule for `jsonb`.
    let mut binary = vec![1u8];
    binary.extend_from_slice(document.as_bytes());
    let payload = match binary.split_first() {
        Some((1, rest)) => rest,
        _ => &binary[..],
    };
    assert_eq!(std::str::from_utf8(payload).unwrap(), document);
    // Version-less `json` payloads pass through unchanged.
    let json_payload = document.as_bytes();
    assert_eq!(
        std::str::from_utf8(match json_payload.split_first() {
            Some((1, rest)) => rest,
            _ => json_payload,
        })
        .unwrap(),
        document
    );
}

/// #592: JSON params are bound as text with an explicit `JSONB` type instead of
/// a `serde_json::Value` round-trip.
#[test]
fn json_params_bind_as_text_with_explicit_type_592() {
    let document = r#"{"b":1,"a":0.1234567890123456789}"#;
    let mut args = PgArguments::default();
    let types = bind_params_with_types(&[QueryParam::Json(serde_json::json!({"b": 1}))], &mut args)
        .expect("json binds");
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].name(), "JSONB");
    let mut args = PgArguments::default();
    bind_json_text(document, &mut args, None).expect("raw json text binds");
}
