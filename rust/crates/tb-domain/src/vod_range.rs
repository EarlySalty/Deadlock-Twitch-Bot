pub const MAX_DURATION_SECONDS: u32 = 600;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const RETENTION_HOURS: i64 = 24;
pub const USAGE: &str = "Nutze !vodclip Start Ende, z. B. !vodclip 55 58 oder !vodclip 1:55:00 1:58:00. Zahlen sind Minuten; Doppelpunkte oder Punkte trennen Minuten und Sekunden bzw. Stunden, Minuten und Sekunden.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VodRange {
    pub start: u32,
    pub end: u32,
}

impl VodRange {
    pub fn parse(args: &str) -> Result<Self, &'static str> {
        let mut parts = args.split_whitespace();
        let start = parts.next().ok_or(USAGE).and_then(timestamp)?;
        let end = parts.next().ok_or(USAGE).and_then(timestamp)?;
        if parts.next().is_some() {
            return Err(USAGE);
        }
        if end <= start {
            return Err("Das Ende muss nach dem Start liegen.");
        }
        if end - start < 60 || end - start > MAX_DURATION_SECONDS {
            return Err("Ein VOD-Ausschnitt muss zwischen 1 und 10 Minuten lang sein.");
        }
        Ok(Self { start, end })
    }
}

fn timestamp(raw: &str) -> Result<u32, &'static str> {
    if raw.len() > 12 || (raw.contains(':') && raw.contains('.')) {
        return Err(USAGE);
    }
    let parts: Vec<u32> = raw
        .split([':', '.'])
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(USAGE);
            }
            part.parse::<u32>().map_err(|_| USAGE)
        })
        .collect::<Result<_, _>>()?;
    let seconds = match parts.as_slice() {
        [minutes] => minutes.checked_mul(60),
        [minutes, seconds] if *seconds < 60 => minutes
            .checked_mul(60)
            .and_then(|value| value.checked_add(*seconds)),
        [hours, minutes, seconds] if *minutes < 60 && *seconds < 60 => hours
            .checked_mul(3600)
            .and_then(|value| value.checked_add(minutes * 60 + seconds)),
        _ => None,
    }
    .ok_or(USAGE)?;
    if seconds > 48 * 3600 {
        return Err("Die Zeitangabe darf höchstens 48 Stunden betragen.");
    }
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_timestamp_grammar() {
        for args in [
            "55 58",
            "55:00 58:00",
            "55.00 58.00",
            "0:55:00 0:58:00",
            "0.55.00 0.58.00",
        ] {
            assert_eq!(
                VodRange::parse(args),
                Ok(VodRange {
                    start: 3300,
                    end: 3480
                })
            );
        }
        for args in [
            "115 118",
            "115:00 118:00",
            "1:55:00 1:58:00",
            "1.55.00 1.58.00",
        ] {
            assert_eq!(
                VodRange::parse(args),
                Ok(VodRange {
                    start: 6900,
                    end: 7080
                })
            );
        }
        assert_eq!(
            VodRange::parse("00:15 01:15"),
            Ok(VodRange { start: 15, end: 75 })
        );
        for args in [
            "",
            "55",
            "58 55",
            "55 55",
            "55 55:59",
            "0 11",
            "55 58 title",
            "55:60 58:00",
            "1:60:00 2:03:00",
            "1.55:00 1:58:00",
            "-1 3",
            "999999999999 999999999999",
            "55..00 58",
            "NaN 3",
            "2880 2881",
        ] {
            assert!(VodRange::parse(args).is_err(), "{args}");
        }
    }
}
