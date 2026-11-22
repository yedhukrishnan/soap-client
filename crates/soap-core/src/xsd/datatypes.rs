//! Lexical checks for the built-in XML Schema datatypes, used by request validation.
//! Types that are not recognised are accepted, so an unusual schema never causes a
//! false error.

use std::cmp::Ordering;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpace {
    Preserve,
    Replace,
    Collapse,
}

/// The `whiteSpace` facet of a built-in type.
pub fn whitespace_of(builtin: &str) -> WhiteSpace {
    match builtin {
        "string" | "anySimpleType" | "anyType" => WhiteSpace::Preserve,
        "normalizedString" => WhiteSpace::Replace,
        _ => WhiteSpace::Collapse,
    }
}

pub fn normalize(v: &str, ws: WhiteSpace) -> String {
    match ws {
        WhiteSpace::Preserve => v.to_string(),
        WhiteSpace::Replace => v.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }).collect(),
        WhiteSpace::Collapse => v.split_ascii_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// Check a (whitespace-normalised) value against a built-in type, by local name.
pub fn check_builtin(builtin: &str, v: &str) -> Result<(), String> {
    let ok = match builtin {
        "boolean" => matches!(v, "true" | "false" | "1" | "0"),
        "decimal" => is_decimal(v),
        "float" | "double" => is_float(v),
        "integer" | "long" | "int" | "short" | "byte" | "nonNegativeInteger" | "positiveInteger"
        | "nonPositiveInteger" | "negativeInteger" | "unsignedLong" | "unsignedInt" | "unsignedShort"
        | "unsignedByte" => return check_integer(builtin, v),
        "dateTime" => is_date_time(v),
        "date" => strip_tz(v).is_some_and(|(d, _)| is_date(d)),
        "time" => strip_tz(v).is_some_and(|(t, _)| is_time(t)),
        "gYear" => strip_tz(v).is_some_and(|(y, _)| is_year(y)),
        "gYearMonth" => strip_tz(v).is_some_and(|(s, _)| {
            s.rsplit_once('-').is_some_and(|(y, m)| is_year(y) && two_digits(m, 1, 12))
        }),
        "gMonth" => strip_tz(v).is_some_and(|(s, _)| s.strip_prefix("--").is_some_and(|m| two_digits(m, 1, 12))),
        "gDay" => strip_tz(v).is_some_and(|(s, _)| s.strip_prefix("---").is_some_and(|d| two_digits(d, 1, 31))),
        "gMonthDay" => strip_tz(v).is_some_and(|(s, _)| {
            s.strip_prefix("--")
                .and_then(|md| md.split_once('-'))
                .is_some_and(|(m, d)| two_digits(m, 1, 12) && two_digits(d, 1, days_in_month(2000, m.parse().unwrap_or(1))))
        }),
        "duration" => is_duration(v),
        "hexBinary" => v.len().is_multiple_of(2) && v.bytes().all(|b| b.is_ascii_hexdigit()),
        "base64Binary" => B64.decode(v.replace(' ', "")).is_ok(),
        "language" => is_language(v),
        "Name" => is_name(v, true),
        "NCName" | "ID" | "IDREF" | "ENTITY" => is_name(v, false),
        "NMTOKEN" => !v.is_empty() && v.chars().all(is_name_char),
        "NMTOKENS" | "IDREFS" | "ENTITIES" => {
            !v.is_empty() && v.split(' ').all(|t| if builtin == "NMTOKENS" { t.chars().all(is_name_char) } else { is_name(t, false) })
        }
        "QName" => match v.split_once(':') {
            Some((p, l)) => is_name(p, false) && is_name(l, false),
            None => is_name(v, false),
        },
        _ => true,
    };
    if ok { Ok(()) } else { Err(format!("{} is not a valid xs:{builtin}", quote(v))) }
}

/// The value's length for `length`/`minLength`/`maxLength`: characters, or octets for binary types.
pub fn length_of(builtin: &str, v: &str) -> Option<usize> {
    match builtin {
        "hexBinary" => Some(v.len() / 2),
        "base64Binary" => B64.decode(v.replace(' ', "")).ok().map(|b| b.len()),
        "NMTOKENS" | "IDREFS" | "ENTITIES" => Some(v.split(' ').filter(|s| !s.is_empty()).count()),
        _ => Some(v.chars().count()),
    }
}

/// Order two values of a numeric built-in type (for min/max facets). `None` if not comparable.
pub fn compare(builtin: &str, a: &str, b: &str) -> Option<Ordering> {
    if !is_numeric(builtin) {
        return None;
    }
    let (x, y) = (parse_num(a)?, parse_num(b)?);
    x.partial_cmp(&y)
}

pub fn is_numeric(builtin: &str) -> bool {
    matches!(
        builtin,
        "decimal" | "float" | "double" | "integer" | "long" | "int" | "short" | "byte" | "nonNegativeInteger"
            | "positiveInteger" | "nonPositiveInteger" | "negativeInteger" | "unsignedLong" | "unsignedInt"
            | "unsignedShort" | "unsignedByte"
    )
}

fn parse_num(s: &str) -> Option<f64> {
    match s {
        "INF" => Some(f64::INFINITY),
        "-INF" => Some(f64::NEG_INFINITY),
        _ if is_float(s) && s != "NaN" => s.parse().ok(),
        _ => None,
    }
}

/// (total digits, fraction digits) of a decimal, ignoring leading/trailing zeros.
pub fn digits(v: &str) -> Option<(usize, usize)> {
    if !is_decimal(v) {
        return None;
    }
    let v = v.trim_start_matches(['+', '-']);
    let (int, frac) = v.split_once('.').unwrap_or((v, ""));
    let int = int.trim_start_matches('0');
    let frac = frac.trim_end_matches('0');
    Some(((int.len() + frac.len()).max(1), frac.len()))
}

/// A value for messages: quoted, shortened if long.
pub fn quote(v: &str) -> String {
    const MAX: usize = 40;
    if v.chars().count() > MAX {
        format!("\"{}…\"", v.chars().take(MAX).collect::<String>())
    } else {
        format!("\"{v}\"")
    }
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_decimal(v: &str) -> bool {
    let v = v.strip_prefix(['+', '-']).unwrap_or(v);
    let (int, frac) = v.split_once('.').unwrap_or((v, ""));
    (int.is_empty() || all_digits(int)) && (frac.is_empty() || all_digits(frac)) && !(int.is_empty() && frac.is_empty())
}

fn is_float(v: &str) -> bool {
    if matches!(v, "INF" | "-INF" | "+INF" | "NaN") {
        return true;
    }
    let (mantissa, exp) = match v.find(['e', 'E']) {
        Some(i) => (&v[..i], Some(&v[i + 1..])),
        None => (v, None),
    };
    is_decimal(mantissa) && exp.is_none_or(|e| all_digits(e.strip_prefix(['+', '-']).unwrap_or(e)))
}

fn check_integer(builtin: &str, v: &str) -> Result<(), String> {
    let digits = v.strip_prefix(['+', '-']).unwrap_or(v);
    if !all_digits(digits) {
        return Err(format!("{} is not a valid xs:{builtin} (whole number expected)", quote(v)));
    }
    let (min, max): (Option<i128>, Option<i128>) = match builtin {
        "long" => (Some(i64::MIN as i128), Some(i64::MAX as i128)),
        "int" => (Some(i32::MIN as i128), Some(i32::MAX as i128)),
        "short" => (Some(i16::MIN as i128), Some(i16::MAX as i128)),
        "byte" => (Some(i8::MIN as i128), Some(i8::MAX as i128)),
        "nonNegativeInteger" => (Some(0), None),
        "positiveInteger" => (Some(1), None),
        "nonPositiveInteger" => (None, Some(0)),
        "negativeInteger" => (None, Some(-1)),
        "unsignedLong" => (Some(0), Some(u64::MAX as i128)),
        "unsignedInt" => (Some(0), Some(u32::MAX as i128)),
        "unsignedShort" => (Some(0), Some(u16::MAX as i128)),
        "unsignedByte" => (Some(0), Some(u8::MAX as i128)),
        _ => (None, None),
    };
    // Values too large for i128 are only possible for the unbounded types.
    let Ok(n) = v.parse::<i128>() else {
        let negative = v.starts_with('-');
        let out_of_range = (negative && min.is_some()) || (!negative && max.is_some());
        return if out_of_range { Err(range_error(builtin, v, min, max)) } else { Ok(()) };
    };
    if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
        return Err(range_error(builtin, v, min, max));
    }
    Ok(())
}

fn range_error(builtin: &str, v: &str, min: Option<i128>, max: Option<i128>) -> String {
    let range = match (min, max) {
        (Some(a), Some(b)) => format!("{a} to {b}"),
        (Some(a), None) => format!("at least {a}"),
        (None, Some(b)) => format!("at most {b}"),
        (None, None) => String::new(),
    };
    format!("{} is out of range for xs:{builtin} ({range})", quote(v))
}

/// Split off an optional timezone (`Z` or `±hh:mm`), validating it.
fn strip_tz(v: &str) -> Option<(&str, bool)> {
    if let Some(s) = v.strip_suffix('Z') {
        return Some((s, true));
    }
    if v.len() > 6 {
        let (s, tz) = v.split_at(v.len() - 6);
        let b = tz.as_bytes();
        if (b[0] == b'+' || b[0] == b'-') && b[3] == b':' {
            let ok = two_digits(&tz[1..3], 0, 14) && two_digits(&tz[4..6], 0, 59);
            return ok.then_some((s, true));
        }
    }
    Some((v, false))
}

fn two_digits(s: &str, min: u32, max: u32) -> bool {
    s.len() == 2 && all_digits(s) && s.parse::<u32>().is_ok_and(|n| (min..=max).contains(&n))
}

fn is_year(y: &str) -> bool {
    let y = y.strip_prefix('-').unwrap_or(y);
    y.len() >= 4 && all_digits(y) && !(y.len() > 4 && y.starts_with('0'))
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_date(d: &str) -> bool {
    let Some((ym, day)) = d.rsplit_once('-') else { return false };
    let Some((y, m)) = ym.rsplit_once('-') else { return false };
    if !is_year(y) || !two_digits(m, 1, 12) {
        return false;
    }
    let (year, month) = (y.parse::<i64>().unwrap_or(2000), m.parse::<u32>().unwrap_or(1));
    two_digits(day, 1, days_in_month(year, month))
}

fn is_time(t: &str) -> bool {
    let parts: Vec<&str> = t.split(':').collect();
    let [h, m, s] = parts[..] else { return false };
    let (sec, frac) = s.split_once('.').unwrap_or((s, ""));
    if !s.contains('.') || all_digits(frac) {
        if h == "24" {
            return m == "00" && sec == "00" && frac.bytes().all(|b| b == b'0');
        }
        return two_digits(h, 0, 23) && two_digits(m, 0, 59) && two_digits(sec, 0, 59);
    }
    false
}

fn is_date_time(v: &str) -> bool {
    let Some((s, _)) = strip_tz(v) else { return false };
    let Some((d, t)) = s.split_once('T') else { return false };
    is_date(d) && is_time(t)
}

fn is_duration(v: &str) -> bool {
    let v = v.strip_prefix('-').unwrap_or(v);
    let Some(rest) = v.strip_prefix('P') else { return false };
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut any = false;
    let mut scan = |s: &str, units: &[char], allow_frac_last: bool| -> bool {
        let mut num = String::new();
        let mut next = 0;
        for c in s.chars() {
            if c.is_ascii_digit() || (c == '.' && allow_frac_last) {
                num.push(c);
                continue;
            }
            let Some(i) = units[next..].iter().position(|u| *u == c) else { return false };
            let is_seconds = allow_frac_last && c == 'S';
            if num.is_empty() || (num.contains('.') && !is_seconds) || (is_seconds && !is_decimal(&num)) {
                return false;
            }
            next += i + 1;
            num.clear();
            any = true;
        }
        num.is_empty()
    };
    let date_ok = scan(date, &['Y', 'M', 'D'], false);
    let time_ok = match time {
        Some(t) => !t.is_empty() && scan(t, &['H', 'M', 'S'], true),
        None => true,
    };
    date_ok && time_ok && any
}

fn is_language(v: &str) -> bool {
    let mut parts = v.split('-');
    let first = parts.next().unwrap_or("");
    let ok_first = (1..=8).contains(&first.len()) && first.bytes().all(|b| b.is_ascii_alphabetic());
    ok_first && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn is_name_start(c: char, colon: bool) -> bool {
    c.is_alphabetic() || c == '_' || (colon && c == ':')
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '\u{B7}')
}

fn is_name(v: &str, colon: bool) -> bool {
    let mut cs = v.chars();
    cs.next().is_some_and(|c| is_name_start(c, colon)) && cs.all(|c| is_name_char(c) && (colon || c != ':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        for ok in ["1", "-1.5", ".5", "5.", "+0"] {
            assert!(check_builtin("decimal", ok).is_ok(), "{ok}");
        }
        for bad in ["", ".", "1,5", "abc", "1e3"] {
            assert!(check_builtin("decimal", bad).is_err(), "{bad}");
        }
        assert!(check_builtin("double", "1e-3").is_ok());
        assert!(check_builtin("double", "INF").is_ok());
        assert!(check_builtin("double", "inf").is_err());
        assert!(check_builtin("int", "2147483647").is_ok());
        assert!(check_builtin("int", "2147483648").unwrap_err().contains("out of range"));
        assert!(check_builtin("unsignedByte", "-1").is_err());
        assert!(check_builtin("integer", "99999999999999999999999999999999999999999").is_ok());
        assert!(check_builtin("positiveInteger", "0").is_err());
        assert!(check_builtin("int", "1.0").is_err());
        assert_eq!(digits("-012.3400"), Some((4, 2)));
    }

    #[test]
    fn dates_and_times() {
        for ok in ["2024-02-29T23:59:59", "2024-01-01T00:00:00.123Z", "2024-01-01T24:00:00+05:30"] {
            assert!(check_builtin("dateTime", ok).is_ok(), "{ok}");
        }
        for bad in ["2023-02-29T00:00:00", "2024-13-01T00:00:00", "2024-01-01", "2024-01-01T25:00:00", "2024-01-01T10:00"] {
            assert!(check_builtin("dateTime", bad).is_err(), "{bad}");
        }
        assert!(check_builtin("date", "2024-06-30Z").is_ok());
        assert!(check_builtin("date", "2024-06-31").is_err());
        assert!(check_builtin("time", "13:20:00").is_ok());
        assert!(check_builtin("gYearMonth", "2024-07").is_ok());
        assert!(check_builtin("gMonthDay", "--02-29").is_ok());
        for ok in ["P1Y2M3DT4H5M6.5S", "PT1H", "-P3D", "P1M"] {
            assert!(check_builtin("duration", ok).is_ok(), "{ok}");
        }
        for bad in ["P", "PT", "P1H", "1Y", "P1.5Y", "PM1Y"] {
            assert!(check_builtin("duration", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn other_types() {
        assert!(check_builtin("boolean", "TRUE").is_err());
        assert!(check_builtin("hexBinary", "0aFF").is_ok());
        assert!(check_builtin("hexBinary", "abc").is_err());
        assert!(check_builtin("base64Binary", "aGVsbG8=").is_ok());
        assert!(check_builtin("base64Binary", "a").is_err());
        assert!(check_builtin("language", "en-US").is_ok());
        assert!(check_builtin("NCName", "a:b").is_err());
        assert!(check_builtin("somethingNew", "anything").is_ok());
        assert_eq!(normalize("  a \n b  ", WhiteSpace::Collapse), "a b");
        assert_eq!(length_of("hexBinary", "0aff"), Some(2));
        assert_eq!(compare("int", "10", "9"), Some(Ordering::Greater));
        assert_eq!(compare("string", "10", "9"), None);
    }
}
