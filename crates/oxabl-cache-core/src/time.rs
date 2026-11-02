//! Unix-second timestamps and UTC day numbers.

/// Seconds in a UTC day.
pub const DAY_SECS: i64 = 86_400;

/// The UTC day number (days since 1970-01-01) of a timestamp.
pub fn day_of(timestamp: i64) -> i64 {
    timestamp.div_euclid(DAY_SECS)
}

/// The day numbers `[first, end)` of a calendar month written `YYYY-MM`.
pub fn month_days(month: &str) -> Option<(i64, i64)> {
    let (year, number) = month.split_once('-')?;
    if year.len() != 4 || number.len() != 2 {
        return None;
    }
    let year: i64 = year.parse().ok()?;
    let number: i64 = number.parse().ok()?;
    if !(1..=12).contains(&number) {
        return None;
    }
    let (next_year, next_month) = if number == 12 {
        (year + 1, 1)
    } else {
        (year, number + 1)
    };
    Some((
        days_from_civil(year, number),
        days_from_civil(next_year, next_month),
    ))
}

fn days_from_civil(year: i64, month: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted = (month + 9) % 12;
    let day_of_year = (153 * shifted + 2) / 5;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The day numbers `[first, end)` of the UTC calendar month containing `day`.
pub fn month_bounds_of_day(day: i64) -> (i64, i64) {
    let (year, month) = civil_from_days(day);
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    (
        days_from_civil(year, month),
        days_from_civil(next_year, next_month),
    )
}

fn civil_from_days(day: i64) -> (i64, i64) {
    let shifted = day + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month)
}
