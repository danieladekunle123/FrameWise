//! Small pure parsers used by the checks. All are unit-tested below.

/// Common DDR4/DDR5 data rates in MT/s. Used to recognise a speed inside a part number.
const KNOWN_SPEEDS: &[u32] = &[
    2133, 2400, 2666, 2933, 3000, 3200, 3333, 3466, 3600, 3733, 3800, 3866, 4000, 4133, 4266,
    4400, 4600, 4800, 5200, 5600, 6000, 6200, 6400, 6600, 6800, 7000, 7200, 7600, 8000, 8200,
    8400,
];

/// Rated (XMP/EXPO) speed encoded in a memory part number, if recognisable.
///
/// Handles four-digit speeds (Corsair `CMK16GX4M2B3200C16`, G.Skill `F4-3600C16-8GVKC`,
/// HyperX `KHX3200C16D4/8GX`) and Kingston Fury's three-digit form (`KF436C16BB/8` = DDR4-3600).
pub fn speed_from_part_number(part: &str) -> Option<u32> {
    let p = part.trim().to_ascii_uppercase();

    // Kingston Fury: KF + generation digit + two speed digits.
    if let Some(rest) = p.strip_prefix("KF") {
        let digits: String = rest.chars().take(3).collect();
        if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit()) {
            let gen = &digits[0..1];
            let code: u32 = digits[1..3].parse().ok()?;
            if gen == "4" || gen == "5" {
                let speed = match code {
                    21 => 2133,
                    26 => 2666,
                    29 => 2933,
                    37 => 3733,
                    c => c * 100,
                };
                if KNOWN_SPEEDS.contains(&speed) {
                    return Some(speed);
                }
            }
        }
    }

    // Any run of exactly four digits that is a known data rate. Take the highest, since kits
    // sometimes include a capacity such as "3200" only by coincidence of being lower.
    let mut best: Option<u32> = None;
    let mut run = String::new();
    for c in p.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            if run.len() == 4 {
                if let Ok(n) = run.parse::<u32>() {
                    if KNOWN_SPEEDS.contains(&n) {
                        best = Some(best.map_or(n, |b: u32| b.max(n)));
                    }
                }
            }
            run.clear();
        }
    }
    best
}

/// Memory channel letter from a module's slot labels, if the firmware names it.
///
/// Recognises `ChannelA-DIMM0`, `Controller0-ChannelB-DIMM1`, `DIMM_A1`, `A2`, `P0 CHANNEL A`.
pub fn channel_letter(device_locator: &str, bank_label: &str) -> Option<char> {
    for source in [device_locator, bank_label] {
        let s = source.to_ascii_uppercase();
        if let Some(idx) = s.find("CHANNEL") {
            let after = s[idx + "CHANNEL".len()..].trim_start_matches([' ', '-', '_']);
            if let Some(c) = after.chars().next() {
                if c.is_ascii_uppercase() {
                    return Some(c);
                }
            }
        }
        for token in s.split(|c: char| !c.is_ascii_alphanumeric()) {
            let b = token.as_bytes();
            if b.len() == 2 && (b'A'..=b'H').contains(&b[0]) && b[1].is_ascii_digit() {
                return Some(b[0] as char);
            }
            // "DIMMA1" style.
            if let Some(rest) = token.strip_prefix("DIMM") {
                let rb = rest.as_bytes();
                if rb.len() == 2 && (b'A'..=b'H').contains(&rb[0]) && rb[1].is_ascii_digit() {
                    return Some(rb[0] as char);
                }
            }
        }
    }
    None
}

/// Days since the Unix epoch for a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parses `yyyymmdd` (the start of a CIM datetime) into days since the epoch.
pub fn yyyymmdd_to_days(s: &str) -> Option<i64> {
    if s.len() < 8 || !s[..8].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: u32 = s[4..6].parse().ok()?;
    let d: u32 = s[6..8].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1990 {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// Formats `yyyymmdd` as `yyyy-mm-dd`.
pub fn pretty_date(s: &str) -> String {
    if s.len() >= 8 {
        format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8])
    } else {
        s.to_string()
    }
}

pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    Some(if v.len() % 2 == 0 { (v[mid - 1] + v[mid]) / 2.0 } else { v[mid] })
}

/// Jitter as the mean absolute difference between consecutive samples (RFC 3550 style).
pub fn jitter(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let sum: f64 = values.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    Some(sum / (values.len() - 1) as f64)
}

pub fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_numbers() {
        assert_eq!(speed_from_part_number("CMK16GX4M2B3200C16"), Some(3200));
        assert_eq!(speed_from_part_number("F4-3600C16-8GVKC"), Some(3600));
        assert_eq!(speed_from_part_number("CMK32GX5M2B5600C36"), Some(5600));
        assert_eq!(speed_from_part_number("KF436C16BB/8"), Some(3600));
        assert_eq!(speed_from_part_number("KF556C40BBK2-32"), Some(5600));
        assert_eq!(speed_from_part_number("KHX3200C16D4/8GX"), Some(3200));
        assert_eq!(speed_from_part_number("  M378A1K43CB2-CTD    "), None);
        assert_eq!(speed_from_part_number(""), None);
    }

    #[test]
    fn channels() {
        assert_eq!(channel_letter("ChannelA-DIMM0", "BANK 0"), Some('A'));
        assert_eq!(channel_letter("Controller0-ChannelB-DIMM1", ""), Some('B'));
        assert_eq!(channel_letter("DIMM_A2", "BANK 1"), Some('A'));
        assert_eq!(channel_letter("DIMM 1", "P0 CHANNEL B"), Some('B'));
        assert_eq!(channel_letter("DIMMB1", ""), Some('B'));
        assert_eq!(channel_letter("DIMM 0", "BANK 0"), None);
    }

    #[test]
    fn dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(yyyymmdd_to_days("20240115000000.000000-000"), Some(days_from_civil(2024, 1, 15)));
        assert_eq!(yyyymmdd_to_days("garbage"), None);
        assert_eq!(pretty_date("20240115"), "2024-01-15");
    }

    #[test]
    fn stats() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(jitter(&[10.0, 20.0, 10.0]), Some(10.0));
        assert_eq!(jitter(&[10.0]), None);
    }
}
