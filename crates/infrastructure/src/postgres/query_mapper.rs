use std::net::{Ipv4Addr, Ipv6Addr};

use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::{CellValue, ColumnMeta, QueryParam, Row};
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgValueFormat;
use sqlx::{Arguments, Column, Row as _, TypeInfo, ValueRef};

pub fn bind_params(params: &[QueryParam], args: &mut PgArguments) -> Result<(), DbError> {
    bind_params_inner(params, args, None)
}

pub(crate) fn bind_params_with_types(
    params: &[QueryParam],
    args: &mut PgArguments,
) -> Result<Vec<sqlx::postgres::PgTypeInfo>, DbError> {
    let mut types = Vec::with_capacity(params.len());
    bind_params_inner(params, args, Some(&mut types))?;
    Ok(types)
}

// cc-scan:allow COMPLEXITY,LONG_FUNCTION — classifier/dispatch ladder — one case per branch
fn bind_params_inner(
    params: &[QueryParam],
    args: &mut PgArguments,
    mut types: Option<&mut Vec<sqlx::postgres::PgTypeInfo>>,
) -> Result<(), DbError> {
    for param in params {
        let result = match param {
            QueryParam::Null => {
                // NULL has no payload or declared domain type. PostgreSQL's
                // Parse OID 0 preserves contextual inference instead of TEXT.
                if let Some(types) = types.as_deref_mut() {
                    types.push(sqlx::postgres::PgTypeInfo::with_oid(sqlx::postgres::types::Oid(0)));
                }
                args.add(Option::<String>::None)
            }
            QueryParam::Bool(v) => bind_value(v, args, types.as_deref_mut()),
            QueryParam::Int64(v) => bind_value(v, args, types.as_deref_mut()),
            QueryParam::Float64(v) => bind_value(v, args, types.as_deref_mut()),
            QueryParam::Decimal(v) => {
                // #590: NUMERIC NaN/Infinity have no BigDecimal form; bind them
                // as text so PostgreSQL casts them with the column type.
                if is_special_decimal(v) {
                    bind_value(v.as_str(), args, types.as_deref_mut())
                } else {
                let decimal = v
                    .parse::<sqlx::types::BigDecimal>()
                    .map_err(|error| DbError::QueryFailed(format!("invalid decimal parameter: {error}")))?;
                bind_value(decimal, args, types.as_deref_mut())
                }
            }
            QueryParam::Text(v) => bind_value(v.as_str(), args, types.as_deref_mut()),
            QueryParam::Bytes(v) => bind_value(v.as_slice(), args, types.as_deref_mut()),
            QueryParam::Uuid(v) => {
                let uuid = uuid::Uuid::parse_str(v)
                    .map_err(|e| DbError::QueryFailed(format!("invalid UUID parameter: {e}")))?;
                bind_value(uuid, args, types.as_deref_mut())
            }
            QueryParam::DateTime(v) => {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(v) {
                    bind_value(dt.with_timezone(&chrono::Utc), args, types.as_deref_mut())
                } else if let Ok(dt) = chrono::DateTime::parse_from_str(v, "%Y-%m-%d %H:%M:%S%z") {
                    bind_value(dt.with_timezone(&chrono::Utc), args, types.as_deref_mut())
                } else if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%d %H:%M:%S%.f") {
                    bind_value(ndt.and_utc(), args, types.as_deref_mut())
                } else if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%d %H:%M:%S") {
                    bind_value(ndt.and_utc(), args, types.as_deref_mut())
                } else if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S%.f") {
                    bind_value(ndt.and_utc(), args, types.as_deref_mut())
                } else if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S") {
                    bind_value(ndt.and_utc(), args, types.as_deref_mut())
                } else if let Ok(date) = chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d") {
                    bind_value(date, args, types.as_deref_mut())
                } else {
                    return Err(DbError::QueryFailed(format!("invalid date/datetime parameter: {v}")));
                }
            }
            QueryParam::Time(v) => match parse_time_parameter(v)? {
                PostgresTimeParameter::Time(time) => bind_value(time, args, types.as_deref_mut()),
                PostgresTimeParameter::TimeWithOffset(time) => bind_value(time, args, types.as_deref_mut()),
            },
            QueryParam::Interval(v) => bind_value(parse_interval_parameter(v)?, args, types.as_deref_mut()),
            QueryParam::Inet(v) => {
                let network = v
                    .parse::<ipnetwork::IpNetwork>()
                    .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL INET parameter: {error}")))?;
                bind_value(network, args, types.as_deref_mut())
            }
            // #592: bind the document's exact text. `sqlx::types::Json` goes
            // through `serde_json`, which rounds large numbers and reorders
            // keys, so saving one edited field rewrote the whole value.
            QueryParam::Json(v) => bind_json_text(&v.to_string(), args, types.as_deref_mut()),
        };
        result.map_err(|e| DbError::QueryFailed(format!("failed to bind parameter: {e}")))?;
    }
    Ok(())
}

/// Capture SQLx's type selection for non-null values after conversion.
/// Callers that only execute do not allocate or collect a second type vector.
/// Bind JSON text with an explicit `jsonb` cast (#592).
fn bind_json_text(
    text: &str,
    args: &mut PgArguments,
    types: Option<&mut Vec<sqlx::postgres::PgTypeInfo>>,
) -> Result<(), sqlx::error::BoxDynError> {
    if let Some(types) = types {
        args.add(text)?;
        types.push(sqlx::postgres::PgTypeInfo::with_name("JSONB"));
        return Ok(());
    }
    args.add(text)?;
    Ok(())
}

fn bind_value<'q, T>(
    value: T,
    args: &mut PgArguments,
    types: Option<&mut Vec<sqlx::postgres::PgTypeInfo>>,
) -> Result<(), sqlx::error::BoxDynError>
where
    T: sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + 'q,
{
    if let Some(types) = types {
        let type_info = value.produces().unwrap_or_else(T::type_info);
        args.add(value)?;
        types.push(type_info);
        Ok(())
    } else {
        args.add(value)
    }
}

enum PostgresTimeParameter {
    Time(chrono::NaiveTime),
    TimeWithOffset(sqlx::postgres::types::PgTimeTz<chrono::NaiveTime, chrono::FixedOffset>),
}

fn parse_time_parameter(value: &str) -> Result<PostgresTimeParameter, DbError> {
    let time_with_offset = parse_time_with_offset_parameter(value);
    if let Ok(time) = time_with_offset {
        return Ok(PostgresTimeParameter::TimeWithOffset(time));
    }

    for format in ["%H:%M:%S%.f", "%H:%M"] {
        if let Ok(time) = chrono::NaiveTime::parse_from_str(value, format) {
            return Ok(PostgresTimeParameter::Time(time));
        }
    }

    Err(DbError::QueryFailed(format!(
        "invalid PostgreSQL TIME parameter: {value}"
    )))
}

fn parse_time_with_offset_parameter(
    value: &str,
) -> Result<sqlx::postgres::types::PgTimeTz<chrono::NaiveTime, chrono::FixedOffset>, DbError> {
    let mut value_with_date = String::with_capacity(value.len() + 11);
    value_with_date.push_str("2001-07-08 ");
    value_with_date.push_str(value);

    for format in ["%Y-%m-%d %H:%M:%S%.f%#z", "%Y-%m-%d %H:%M:%S%#z", "%Y-%m-%d %H:%M%#z"] {
        if let Ok(parsed) = chrono::DateTime::parse_from_str(&value_with_date, format) {
            return Ok(sqlx::postgres::types::PgTimeTz {
                time: parsed.time(),
                offset: *parsed.offset(),
            });
        }
    }

    Err(DbError::QueryFailed(format!(
        "invalid PostgreSQL TIMETZ parameter: {value}"
    )))
}

// cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
fn parse_interval_parameter(value: &str) -> Result<sqlx::postgres::types::PgInterval, DbError> {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(DbError::QueryFailed(
            "invalid PostgreSQL INTERVAL parameter: empty value".into(),
        ));
    }

    let mut interval = sqlx::postgres::types::PgInterval::default();
    let mut token_index = 0;
    while token_index < tokens.len() {
        let token = tokens[token_index];
        if token.contains(':') {
            if token_index + 1 != tokens.len() {
                return Err(DbError::QueryFailed(format!(
                    "invalid PostgreSQL INTERVAL parameter: unexpected token after {token}"
                )));
            }
            let component = parse_interval_time(token)?;
            interval.microseconds = interval
                .microseconds
                .checked_add(component)
                .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL time value is out of range".into()))?;
            token_index += 1;
            continue;
        }

        let unit = tokens.get(token_index + 1).ok_or_else(|| {
            DbError::QueryFailed(format!(
                "invalid PostgreSQL INTERVAL parameter: missing unit after {token}"
            ))
        })?;
        add_interval_component(&mut interval, token, unit)?;
        token_index += 2;
    }

    Ok(interval)
}

fn add_interval_component(
    interval: &mut sqlx::postgres::types::PgInterval,
    magnitude: &str,
    unit: &str,
) -> Result<(), DbError> {
    let normalized_unit = unit.to_ascii_lowercase();
    let target = match normalized_unit.as_str() {
        "year" | "years" => IntervalComponent::Months(12),
        "mon" | "mons" | "month" | "months" => IntervalComponent::Months(1),
        "week" | "weeks" => IntervalComponent::Days(7),
        "day" | "days" => IntervalComponent::Days(1),
        "hour" | "hours" => IntervalComponent::Microseconds(3_600_000_000),
        "minute" | "minutes" => IntervalComponent::Microseconds(60_000_000),
        "second" | "seconds" => IntervalComponent::Microseconds(1_000_000),
        _ => {
            return Err(DbError::QueryFailed(format!(
                "invalid PostgreSQL INTERVAL unit: {unit}"
            )))
        }
    };

    match target {
        IntervalComponent::Months(multiplier) => {
            let component = parse_integer_component(magnitude, "months")?;
            let months = component
                .checked_mul(multiplier)
                .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL months value is out of range".into()))?;
            interval.months = interval
                .months
                .checked_add(months)
                .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL months value is out of range".into()))?;
        }
        IntervalComponent::Days(multiplier) => {
            let component = parse_integer_component(magnitude, "days")?;
            let days = component
                .checked_mul(multiplier)
                .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL days value is out of range".into()))?;
            interval.days = interval
                .days
                .checked_add(days)
                .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL days value is out of range".into()))?;
        }
        IntervalComponent::Microseconds(multiplier) => {
            let component = parse_scaled_component(magnitude, multiplier)?;
            interval.microseconds = interval
                .microseconds
                .checked_add(component)
                .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL time value is out of range".into()))?;
        }
    }

    Ok(())
}

enum IntervalComponent {
    Months(i32),
    Days(i32),
    Microseconds(i64),
}

fn parse_integer_component(value: &str, component: &str) -> Result<i32, DbError> {
    value.parse::<i32>().map_err(|error| {
        DbError::QueryFailed(format!(
            "invalid PostgreSQL INTERVAL {component} value {value}: {error}"
        ))
    })
}

// cc-scan:allow HARD_COMPLEXITY — NUMERIC text parser: sign/digits/exponent cases handled as a ladder
fn parse_scaled_component(value: &str, multiplier: i64) -> Result<i64, DbError> {
    let (sign, unsigned) = match value.as_bytes().first() {
        Some(b'-') => (-1i64, &value[1..]),
        Some(b'+') => (1i64, &value[1..]),
        _ => (1i64, value),
    };
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if fraction.len() > 6 || fraction.bytes().any(|byte| !byte.is_ascii_digit()) {
        return Err(DbError::QueryFailed(format!(
            "invalid PostgreSQL INTERVAL fractional value: {value}"
        )));
    }
    let integer = if integer.is_empty() {
        0
    } else {
        integer
            .parse::<i64>()
            .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL INTERVAL value {value}: {error}")))?
    };
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        let fraction_length = u32::try_from(fraction.len())
            .map_err(|_| DbError::QueryFailed("PostgreSQL INTERVAL fractional value is too long".into()))?;
        let denominator = 10i64.pow(fraction_length);
        if multiplier % denominator != 0 {
            return Err(DbError::QueryFailed(
                "PostgreSQL INTERVAL precision exceeds microseconds".into(),
            ));
        }
        let numerator = fraction.parse::<i64>().map_err(|error| {
            DbError::QueryFailed(format!("invalid PostgreSQL INTERVAL fractional value {value}: {error}"))
        })?;
        numerator
            .checked_mul(multiplier / denominator)
            .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL value is out of range".into()))?
    };
    let scaled_integer = integer
        .checked_mul(multiplier)
        .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL value is out of range".into()))?;
    sign.checked_mul(
        scaled_integer
            .checked_add(fraction_value)
            .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL value is out of range".into()))?,
    )
    .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL value is out of range".into()))
}

fn parse_interval_time(value: &str) -> Result<i64, DbError> {
    let (sign, unsigned) = match value.as_bytes().first() {
        Some(b'-') => (-1i64, &value[1..]),
        Some(b'+') => (1i64, &value[1..]),
        _ => (1i64, value),
    };
    let fields: Vec<&str> = unsigned.split(':').collect();
    if fields.len() != 3 {
        return Err(DbError::QueryFailed(format!(
            "invalid PostgreSQL INTERVAL time value: {value}"
        )));
    }
    let hours = fields[0]
        .parse::<i64>()
        .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL INTERVAL hour value {value}: {error}")))?;
    let minutes = fields[1]
        .parse::<i64>()
        .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL INTERVAL minute value {value}: {error}")))?;
    if !(0..60).contains(&minutes) {
        return Err(DbError::QueryFailed(format!(
            "invalid PostgreSQL INTERVAL minute value: {value}"
        )));
    }
    let seconds = parse_scaled_component(fields[2], 1_000_000)?;
    if !(0..60_000_000).contains(&seconds) {
        return Err(DbError::QueryFailed(format!(
            "invalid PostgreSQL INTERVAL second value: {value}"
        )));
    }
    let total = hours
        .checked_mul(3_600_000_000)
        .and_then(|value| value.checked_add(minutes * 60_000_000))
        .and_then(|value| value.checked_add(seconds))
        .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL time value is out of range".into()))?;
    sign.checked_mul(total)
        .ok_or_else(|| DbError::QueryFailed("PostgreSQL INTERVAL time value is out of range".into()))
}

pub fn columns_from_describe(describe: &sqlx::Describe<sqlx::Postgres>) -> Vec<ColumnMeta> {
    describe
        .columns()
        .iter()
        .enumerate()
        .map(|(i, col)| ColumnMeta {
            name: col.name().to_string(),
            data_type: col.type_info().name().to_string(),
            nullable: describe.nullable(i).unwrap_or(true),
        })
        .collect()
}

pub fn map_row(row: &sqlx::postgres::PgRow, columns: &[ColumnMeta]) -> Result<Row, DbError> {
    let mut cells = Vec::with_capacity(columns.len());
    for (i, col) in columns.iter().enumerate() {
        let raw = row.try_get_raw(i).map_err(crate::error::from_sqlx)?;
        let cell = if raw.is_null() {
            CellValue::Null
        } else {
            decode_cell(row, i, &col.data_type)?
        };
        cells.push(cell);
    }
    Ok(Row(cells))
}

// cc-scan:allow HUGE_FUNCTION — flat type-name → decoder dispatch; each arm reads one row cell
fn decode_cell(row: &sqlx::postgres::PgRow, i: usize, data_type: &str) -> Result<CellValue, DbError> {
    let dt_upper = data_type.to_uppercase();
    match dt_upper.as_str() {
        "BOOL" => row
            .try_get::<bool, _>(i)
            .map(CellValue::Bool)
            .map_err(crate::error::from_sqlx),
        "INT2" => row
            .try_get::<i16, _>(i)
            .map(|v| CellValue::Int64(v as i64))
            .map_err(crate::error::from_sqlx),
        "INT4" => row
            .try_get::<i32, _>(i)
            .map(|v| CellValue::Int64(v as i64))
            .map_err(crate::error::from_sqlx),
        "INT8" => row
            .try_get::<i64, _>(i)
            .map(CellValue::Int64)
            .map_err(crate::error::from_sqlx),
        "OID" => row
            .try_get::<sqlx::postgres::types::Oid, _>(i)
            .map(|v| CellValue::Int64(v.0 as i64))
            .map_err(crate::error::from_sqlx),
        "FLOAT4" => row
            .try_get::<f32, _>(i)
            .map(|v| CellValue::Float64(v as f64))
            .map_err(crate::error::from_sqlx),
        "FLOAT8" => row
            .try_get::<f64, _>(i)
            .map(CellValue::Float64)
            .map_err(crate::error::from_sqlx),
        "NUMERIC" | "DECIMAL" => decode_numeric(row, i),
        "UUID" => row
            .try_get::<uuid::Uuid, _>(i)
            .map(|v| CellValue::Uuid(v.to_string()))
            .map_err(crate::error::from_sqlx),
        // The temporal classes keep dedicated variants, so a consumer never has to
        // parse the string to learn whether an instant or a wall clock arrived (#52).
        "TIMESTAMPTZ" => row
            .try_get::<chrono::DateTime<chrono::Utc>, _>(i)
            .map(|v| CellValue::TimestampTz(format_utc_instant(v)))
            .map_err(crate::error::from_sqlx),
        "TIMESTAMP" => row
            .try_get::<chrono::NaiveDateTime, _>(i)
            .map(|v| CellValue::Timestamp(format_naive_timestamp(v)))
            .map_err(crate::error::from_sqlx),
        "DATE" => row
            .try_get::<chrono::NaiveDate, _>(i)
            .map(|v| CellValue::Date(v.to_string()))
            .map_err(crate::error::from_sqlx),
        "TIME" => row
            .try_get::<chrono::NaiveTime, _>(i)
            .map(|v| CellValue::Time(v.to_string()))
            .map_err(crate::error::from_sqlx),
        "TIMETZ" => row
            .try_get::<sqlx::postgres::types::PgTimeTz<chrono::NaiveTime, chrono::FixedOffset>, _>(i)
            .map(|v| CellValue::TimeTz(format_time_with_offset(v.time, v.offset)))
            .map_err(crate::error::from_sqlx),
        "INTERVAL" => row
            .try_get::<sqlx::postgres::types::PgInterval, _>(i)
            .map(|v| CellValue::Interval(format_interval(v)))
            .map_err(crate::error::from_sqlx),
        // #591: arrays, ranges, money, point and composites arrive in binary and
        // used to fall through to the opaque `\x` hex branch. Decoding them into
        // their canonical PostgreSQL text makes the grid readable and editable.
        "INET" | "CIDR" => decode_inet(row, i),
        "MONEY" | "POINT" | "LSEG" | "LINE" | "BOX" | "PATH" | "POLYGON" | "CIRCLE" | "MACADDR" | "MACADDR8"
        | "BIT" | "VARBIT" | "TSVECTOR" | "TSQUERY" | "INT4RANGE" | "INT8RANGE" | "NUMRANGE" | "TSRANGE"
        | "TSTZRANGE" | "DATERANGE" | "PG_LSN" | "TXID_SNAPSHOT" | "PG_SNAPSHOT" => {
            decode_textual_value(row, i, data_type)
        }
        _ if dt_upper.ends_with("[]") => decode_array(row, i, data_type),
        // #592: keep the server's exact text. Decoding through
        // `serde_json::Value` rounded big numbers, reordered object keys and
        // rewrote the document on save; the raw payload round-trips verbatim.
        "JSON" | "JSONB" => decode_json_text(row, i, data_type),
        "BYTEA" => row
            .try_get::<Vec<u8>, _>(i)
            .map(CellValue::Bytes)
            .map_err(crate::error::from_sqlx),
        _ => decode_textual_value(row, i, data_type),
    }
}

/// Fallback decoder for every class without an explicit arm above (enum labels,
/// domains, arrays, ranges, and other provider-specific types).
///
/// The wire format decides how a value may be represented, and getting this wrong
/// is not cosmetic: `as_str` does not check the format, so a binary payload that
/// happened to be valid UTF-8 (an `int4range`, a `text[]`, a `tsvector`) was served
/// as mojibake text, while one that was not (`money`, a composite record) failed the
/// whole query and took the readable columns of the row down with it.
///
/// So: a text-format value is the provider's canonical text; a binary-format value
/// is text only when its column type says the payload *is* text (a character type or
/// an enum label), and is otherwise kept byte-exact — the UI renders those as
/// read-only `\x` hex, which cannot be mistaken for a value.
fn decode_textual_value(row: &sqlx::postgres::PgRow, i: usize, data_type: &str) -> Result<CellValue, DbError> {
    let raw = row.try_get_raw(i).map_err(crate::error::from_sqlx)?;
    match raw.format() {
        PgValueFormat::Text => {
            let value = raw.as_str().map(str::to_owned).map_err(|error| {
                DbError::QueryFailed(format!("cannot decode PostgreSQL {data_type} as text: {error}"))
            })?;
            Ok(CellValue::Text(value))
        }
        PgValueFormat::Binary if binary_payload_is_text(row, i) => {
            let value = raw.as_str().map(str::to_owned).map_err(|error| {
                DbError::QueryFailed(format!("cannot decode PostgreSQL {data_type} as text: {error}"))
            })?;
            Ok(CellValue::Text(value))
        }
        PgValueFormat::Binary => raw
            .as_bytes()
            .map(|bytes| CellValue::Bytes(bytes.to_vec()))
            .map_err(|error| DbError::QueryFailed(format!("cannot decode PostgreSQL {data_type} as bytes: {error}"))),
    }
}

/// Whether a binary-format payload is the UTF-8 text of its value rather than an
/// opaque encoding. sqlx resolves the column's own type from `pg_type`, so the kind
/// answers this exactly for enums (arbitrary type name, label payload), domains (the
/// base type decides) and the structured classes that are *not* text.
fn binary_payload_is_text(row: &sqlx::postgres::PgRow, i: usize) -> bool {
    fn kind_is_text(kind: &sqlx::postgres::PgTypeKind) -> bool {
        match kind {
            sqlx::postgres::PgTypeKind::Enum(_) => true,
            sqlx::postgres::PgTypeKind::Domain(base) => {
                kind_is_text(base.kind()) || is_text_like_type_name(base.name())
            }
            _ => false,
        }
    }

    let type_info = row.columns()[i].type_info();
    kind_is_text(type_info.kind()) || is_text_like_type_name(type_info.name())
}

/// Character types whose binary payload is the value's UTF-8 text (PostgreSQL sends
/// no length prefix or framing for them, unlike ranges, arrays and composites).
fn is_text_like_type_name(name: &str) -> bool {
    let upper = name.trim().to_uppercase();
    let base = upper.split('(').next().unwrap_or(&upper).trim();
    matches!(
        base,
        "TEXT" | "VARCHAR" | "CHARACTER VARYING" | "BPCHAR" | "CHAR" | "CHARACTER" | "NAME" | "XML" | "CITEXT"
    )
}

/// Exact JSON/JSONB text (#592).
///
/// `jsonb` binary values carry a one-byte version header before the UTF-8 text;
/// `json` binary is the text itself. Numbers and key order are preserved because
/// nothing is parsed into `serde_json::Value` on the way through.
fn decode_json_text(row: &sqlx::postgres::PgRow, i: usize, data_type: &str) -> Result<CellValue, DbError> {
    let raw = row.try_get_raw(i).map_err(crate::error::from_sqlx)?;
    let bytes = match raw.format() {
        PgValueFormat::Text => {
            return raw
                .as_str()
                .map(|text| CellValue::Text(text.to_owned()))
                .map_err(|error| DbError::QueryFailed(format!("cannot decode PostgreSQL {data_type}: {error}")));
        }
        PgValueFormat::Binary => raw
            .as_bytes()
            .map_err(|error| DbError::QueryFailed(format!("cannot decode PostgreSQL {data_type}: {error}")))?,
    };
    // `jsonb` version 1 is the only version PostgreSQL emits.
    let payload = match bytes.split_first() {
        Some((1, rest)) => rest,
        _ => bytes,
    };
    String::from_utf8(payload.to_vec())
        .map(CellValue::Text)
        .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL {data_type} text: {error}")))
}

/// Render a PostgreSQL array (#591).
///
/// sqlx only implements `Decode` for a known element type, so the payload is
/// rendered from the wire format instead: a binary array (dimension header +
/// element OIDs + length-prefixed values) becomes `{a,b}` text via
/// [`format_binary_array`], and a text-format array is passed through.
fn decode_array(row: &sqlx::postgres::PgRow, i: usize, data_type: &str) -> Result<CellValue, DbError> {
    let raw = row.try_get_raw(i).map_err(crate::error::from_sqlx)?;
    match raw.format() {
        PgValueFormat::Text => raw
            .as_str()
            .map(|text| CellValue::Text(text.to_owned()))
            .map_err(|error| DbError::QueryFailed(format!("cannot decode PostgreSQL {data_type} array: {error}"))),
        PgValueFormat::Binary => raw
            .as_bytes()
            .map_err(|error| DbError::QueryFailed(format!("cannot decode PostgreSQL {data_type} array: {error}")))
            .and_then(|bytes| {
                format_binary_array(bytes).map(CellValue::Text).ok_or_else(|| {
                    DbError::QueryFailed(format!("invalid PostgreSQL {data_type} array binary value"))
                })
            }),
    }
}

/// `{a,b,c}` from a binary array payload; `None` when the framing is invalid.
///
/// Nested arrays are flattened into one brace level because the grid renders a
/// scalar cell, and a NULL element renders as the bare `NULL` token PostgreSQL
/// itself emits for text output.
fn format_binary_array(bytes: &[u8]) -> Option<String> {
    let mut offset = 0;
    let dimensions = read_u32(bytes, &mut offset)?;
    let _has_nulls = read_u32(bytes, &mut offset)?;
    let _element_oid = read_u32(bytes, &mut offset)?;
    let total = array_element_count(bytes, &mut offset, dimensions)?;
    let mut values = Vec::new();
    for _ in 0..total {
        values.push(read_array_element(bytes, &mut offset)?);
    }
    Some(format!("{{{}}}", values.join(",")))
}

/// Element count from the dimension header (one header per dimension).
fn array_element_count(bytes: &[u8], offset: &mut usize, dimensions: u32) -> Option<usize> {
    let mut total: usize = 1;
    for _ in 0..dimensions {
        let length = read_u32(bytes, offset)? as usize;
        let _lower_bound = read_u32(bytes, offset)?;
        total = total.checked_mul(length.max(1))?;
    }
    Some(total)
}

/// One length-prefixed element; `-1` is the NULL sentinel.
fn read_array_element(bytes: &[u8], offset: &mut usize) -> Option<String> {
    let length = read_u32(bytes, offset)? as i32;
    if length < 0 {
        return Some("NULL".to_owned());
    }
    let end = offset.checked_add(length as usize)?;
    let payload = bytes.get(*offset..end)?;
    *offset = end;
    format_array_element(payload)
}


/// One array element as array-literal text (#591).
///
/// Elements that would break the literal (empty, containing a delimiter/brace/
/// quote/backslash, or spelled like the NULL token) are double-quoted.
fn format_array_element(payload: &[u8]) -> Option<String> {
    let text = String::from_utf8(payload.to_vec()).ok()?;
    let needs_quotes = text.is_empty()
        || text.contains([',', '{', '}', '"', '\\'])
        || text.eq_ignore_ascii_case("null");
    if !needs_quotes {
        return Some(text);
    }
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!("\"{escaped}\""))
}


fn read_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let slice = bytes.get(*offset..*offset + 4)?;
    *offset += 4;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn decode_numeric(row: &sqlx::postgres::PgRow, i: usize) -> Result<CellValue, DbError> {
    let raw = row.try_get_raw(i).map_err(crate::error::from_sqlx)?;

    let value = match raw.format() {
        PgValueFormat::Text => raw
            .as_str()
            .map(str::to_owned)
            .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL NUMERIC text: {error}"))),
        PgValueFormat::Binary => raw
            .as_bytes()
            .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL NUMERIC binary value: {error}")))
            .and_then(|bytes| {
                decode_binary_numeric(bytes)
                    .ok_or_else(|| DbError::QueryFailed("invalid PostgreSQL NUMERIC binary value".into()))
            }),
    };

    value.map(CellValue::Decimal)
}

fn decode_inet(row: &sqlx::postgres::PgRow, i: usize) -> Result<CellValue, DbError> {
    let raw = row.try_get_raw(i).map_err(crate::error::from_sqlx)?;
    let value = match raw.format() {
        PgValueFormat::Text => raw
            .as_str()
            .map(str::to_owned)
            .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL INET text: {error}"))),
        PgValueFormat::Binary => raw
            .as_bytes()
            .map_err(|error| DbError::QueryFailed(format!("invalid PostgreSQL INET binary value: {error}")))
            .and_then(decode_binary_inet),
    }?;
    Ok(CellValue::Inet(value))
}

fn decode_binary_inet(bytes: &[u8]) -> Result<String, DbError> {
    if bytes.len() < 4 {
        return Err(DbError::QueryFailed("invalid PostgreSQL INET binary value".into()));
    }
    let family = bytes[0];
    let prefix = bytes[1];
    let address_len = usize::from(bytes[3]);
    let expected_len = 4usize
        .checked_add(address_len)
        .ok_or_else(|| DbError::QueryFailed("invalid PostgreSQL INET address length".into()))?;
    if bytes.len() != expected_len {
        return Err(DbError::QueryFailed("invalid PostgreSQL INET binary length".into()));
    }
    let address = &bytes[4..];
    match (family, address_len) {
        (2, 4) => Ok(format!(
            "{}/{}",
            Ipv4Addr::new(address[0], address[1], address[2], address[3]),
            prefix
        )),
        (3, 16) => {
            let octets: [u8; 16] = address
                .try_into()
                .map_err(|_| DbError::QueryFailed("invalid PostgreSQL INET IPv6 length".into()))?;
            Ok(format!("{}/{}", Ipv6Addr::from(octets), prefix))
        }
        _ => Err(DbError::QueryFailed(
            "unsupported PostgreSQL INET address family".into(),
        )),
    }
}

/// Canonical string for a `timestamptz`: the instant in UTC, marked as UTC.
///
/// `Z` rather than `+00:00` so the marker is comparable with what the frontend and
/// the copy/export paths expect from an absolute instant.
fn format_utc_instant(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Canonical string for a `timestamp`: a wall-clock reading with no timezone.
///
/// PostgreSQL's `timestamp` carries no offset, so attaching one here would invent an
/// instant the database never stored (and would move the reading with the session or
/// OS timezone). Microseconds are spelled out to match the canonical contract.
fn format_naive_timestamp(value: chrono::NaiveDateTime) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

fn format_time_with_offset(time: chrono::NaiveTime, offset: chrono::FixedOffset) -> String {
    let seconds = offset.local_minus_utc();
    let sign = if seconds >= 0 { '+' } else { '-' };
    let absolute = seconds.unsigned_abs();
    format!("{time}{sign}{:02}:{:02}", absolute / 3_600, (absolute % 3_600) / 60)
}

fn format_interval(interval: sqlx::postgres::types::PgInterval) -> String {
    let mut parts = Vec::new();
    if interval.months != 0 {
        parts.push(format!("{} mons", interval.months));
    }
    if interval.days != 0 {
        parts.push(format!("{} days", interval.days));
    }
    if interval.microseconds != 0 {
        let negative = interval.microseconds < 0;
        let absolute = interval.microseconds.unsigned_abs();
        let hours = absolute / 3_600_000_000;
        let minutes = (absolute % 3_600_000_000) / 60_000_000;
        let seconds = (absolute % 60_000_000) / 1_000_000;
        let micros = absolute % 1_000_000;
        let time = if micros == 0 {
            format!("{hours:02}:{minutes:02}:{seconds:02}")
        } else {
            format!("{hours:02}:{minutes:02}:{seconds:02}.{micros:06}")
        };
        parts.push(if negative { format!("-{time}") } else { time });
    }
    if parts.is_empty() {
        "00:00:00".into()
    } else {
        parts.join(" ")
    }
}

// cc-scan:allow LINE_TOO_LONG — numeric literal formatting must not wrap
/// `true` for the NUMERIC special values PostgreSQL 14+ accepts (#590).
///
/// `BigDecimal` cannot represent them, so they are bound as text and cast by
/// the database instead of failing the whole statement.
fn is_special_decimal(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "nan" | "infinity" | "+infinity" | "-infinity" | "inf" | "+inf" | "-inf"
    )
}

// cc-scan:allow HARD_COMPLEXITY,HUGE_FUNCTION — one arm per numeric sign; splitting loses the literal
fn decode_binary_numeric(bytes: &[u8]) -> Option<String> {
    let mut offset = 0;
    let digit_count = usize::from(read_u16(bytes, &mut offset)?);
    let weight = i32::from(read_i16(bytes, &mut offset)?);
    let sign = read_u16(bytes, &mut offset)?;
    let scale = usize::try_from(read_i16(bytes, &mut offset)?).ok()?;
    let expected_len = 8usize.checked_add(digit_count.checked_mul(2)?)?;
    if bytes.len() != expected_len {
        return None;
    }
    // #590: PostgreSQL 14 added numeric Infinity/-Infinity (sign 0xD000/0xF000).
    // Rejecting them returned `None`, which failed the *whole* result set.
    let special = match sign {
        0xC000 => Some("NaN"),
        0xD000 => Some("Infinity"),
        0xF000 => Some("-Infinity"),
        _ => None,
    };
    if let Some(text) = special {
        // A special value carries no digits; anything else is malformed.
        return (digit_count == 0).then(|| text.to_string());
    }
    let negative = match sign {
        0x0000 => false,
        0x4000 => true,
        _ => return None,
    };

    let mut digits = String::new();
    for _ in 0..digit_count {
        let digit = read_u16(bytes, &mut offset)?;
        if digit >= 10_000 {
            return None;
        }
        use std::fmt::Write;
        write!(&mut digits, "{digit:04}").ok()?;
    }

    let decimal_position = weight.checked_add(1)?.checked_mul(4)?;
    let (mut integer, mut fraction) = if decimal_position <= 0 {
        (
            String::from("0"),
            "0".repeat(usize::try_from(decimal_position.unsigned_abs()).ok()?) + &digits,
        )
    } else {
        let position = usize::try_from(decimal_position).ok()?;
        if position >= digits.len() {
            (
                format!("{digits}{}", "0".repeat(position - digits.len())),
                String::new(),
            )
        } else {
            (digits[..position].to_owned(), digits[position..].to_owned())
        }
    };

    let trimmed_integer = integer.trim_start_matches('0');
    integer = if trimmed_integer.is_empty() {
        "0".into()
    } else {
        trimmed_integer.into()
    };

    if fraction.len() > scale && fraction[scale..].bytes().all(|byte| byte == b'0') {
        fraction.truncate(scale);
    } else if fraction.len() < scale {
        fraction.push_str(&"0".repeat(scale - fraction.len()));
    }

    let mut result = String::with_capacity(integer.len() + fraction.len() + 2);
    if negative {
        result.push('-');
    }
    result.push_str(&integer);
    if !fraction.is_empty() {
        result.push('.');
        result.push_str(&fraction);
    }
    Some(result)
}

fn read_u16(bytes: &[u8], offset: &mut usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let value = u16::from_be_bytes(bytes.get(*offset..end)?.try_into().ok()?);
    *offset = end;
    Some(value)
}

fn read_i16(bytes: &[u8], offset: &mut usize) -> Option<i16> {
    Some(i16::from_be_bytes(read_u16(bytes, offset)?.to_be_bytes()))
}

#[cfg(test)]
#[path = "tests/query_mapper_tests.rs"]
mod tests;
