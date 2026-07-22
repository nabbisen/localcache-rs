//! Panic-free text formatting for database-derived CLI output.

/// Keep the rightmost `max - 1` Unicode scalar values and prefix an ellipsis.
pub(crate) fn truncate(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }

    let scalar_count = value.chars().count();
    if scalar_count <= max {
        return value.to_owned();
    }
    if max == 1 {
        return "…".to_owned();
    }

    let suffix = value
        .chars()
        .skip(scalar_count - (max - 1))
        .collect::<String>();
    format!("…{suffix}")
}

/// Format any signed Unix timestamp in seconds in the proleptic Gregorian
/// calendar without narrowing or unsigned conversion.
pub(crate) fn format_unix_seconds(timestamp: i64) -> String {
    format_seconds(i128::from(timestamp))
}

/// Format a signed Unix timestamp in nanoseconds with exactly nine fractional
/// digits. Euclidean division keeps the fraction nonnegative before the epoch.
pub(crate) fn format_unix_nanoseconds(timestamp: i64) -> String {
    let timestamp = i128::from(timestamp);
    let seconds = timestamp.div_euclid(1_000_000_000);
    let nanoseconds = timestamp.rem_euclid(1_000_000_000);
    format!("{}.{nanoseconds:09}", format_seconds(seconds))
}

fn format_seconds(timestamp: i128) -> String {
    let days = timestamp.div_euclid(86_400);
    let seconds_in_day = timestamp.rem_euclid(86_400);
    let hour = seconds_in_day / 3_600;
    let minute = (seconds_in_day / 60) % 60;
    let second = seconds_in_day % 60;
    let (year, month, day) = civil_from_days(days);
    let year = format_year(year);

    format!("{year}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// Howard Hinnant's civil-from-days algorithm, expressed with Euclidean
/// division and `i128` intermediates so the complete `i64` seconds domain is
/// representable.
fn civil_from_days(days_since_epoch: i128) -> (i128, i128, i128) {
    let shifted = days_since_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

fn format_year(year: i128) -> String {
    match year {
        0..=9_999 => format!("{year:04}"),
        -9_999..=-1 => format!("-{:04}", -year),
        _ => year.to_string(),
    }
}

#[cfg(test)]
#[path = "text/tests.rs"]
mod tests;
