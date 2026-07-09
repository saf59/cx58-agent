use chrono::{Datelike, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::Europe::Berlin;

#[derive(Debug, thiserror::Error)]
pub enum ReportDatetimeError {
    #[error("Invalid berlin_datetime format")]
    InvalidFormat,
    #[error("Invalid berlin_datetime year")]
    InvalidYear,
    #[error("Invalid berlin_datetime local time")]
    InvalidLocalTime,
}

pub fn parse_berlin_datetime_as_utc_naive(
    berlin_datetime: &str,
) -> Result<NaiveDateTime, ReportDatetimeError> {
    let local = NaiveDateTime::parse_from_str(berlin_datetime, "%d.%m.%Y %H:%M:%S")
        .map_err(|_| ReportDatetimeError::InvalidFormat)?;
    if local.year() < 2000 || local.year() > 2100 {
        return Err(ReportDatetimeError::InvalidYear);
    }

    match Berlin.from_local_datetime(&local) {
        LocalResult::Single(datetime) => Ok(datetime.naive_utc()),
        LocalResult::Ambiguous(earliest, _) => Ok(earliest.naive_utc()),
        LocalResult::None => Err(ReportDatetimeError::InvalidLocalTime),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn converts_berlin_summer_time_to_utc_naive() {
        let utc = parse_berlin_datetime_as_utc_naive("09.07.2026 20:00:00").unwrap();

        assert_eq!(
            utc,
            NaiveDate::from_ymd_opt(2026, 7, 9)
                .unwrap()
                .and_hms_opt(18, 0, 0)
                .unwrap()
        );
    }

    #[test]
    fn converts_berlin_winter_time_to_utc_naive() {
        let utc = parse_berlin_datetime_as_utc_naive("09.01.2026 20:00:00").unwrap();

        assert_eq!(
            utc,
            NaiveDate::from_ymd_opt(2026, 1, 9)
                .unwrap()
                .and_hms_opt(19, 0, 0)
                .unwrap()
        );
    }
}
