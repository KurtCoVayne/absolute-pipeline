//! Time (A-D7, PL-4.2): nanoseconds since the Unix epoch, UTC, `i64`; the
//! fixity time of an X post from its snowflake id; ISO-8601 for people.

/// Nanoseconds since the Unix epoch, UTC.
pub type Time = i64;

pub const NS_PER_MS: i64 = 1_000_000;
pub const NS_PER_S: i64 = 1_000_000_000;

/// X's snowflake epoch, milliseconds since the Unix epoch.
pub const X_EPOCH_MS: i64 = 1_288_834_974_657;

/// The creation time an X post id encodes (Definition PL-4.1):
/// `(id >> 22) + 1288834974657` ms. An edit on X makes a new post with a new
/// id, so this is the instant after which the content could not change.
pub fn snowflake_created(post_id: u64) -> Time {
    (((post_id >> 22) as i64) + X_EPOCH_MS) * NS_PER_MS
}

/// Wall-clock now, nanoseconds.
pub fn now() -> Time {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    d.as_nanos() as i64
}

/// Days from the civil date (proleptic Gregorian), Howard Hinnant's algorithm.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// ISO-8601 with nanoseconds when they are not zero, as `abt --json` prints
/// a `Time`: `2026-09-18T19:12:36Z` or `2026-09-18T19:12:36.123456789Z`.
pub fn iso(t: Time) -> String {
    let secs = t.div_euclid(NS_PER_S);
    let nanos = t.rem_euclid(NS_PER_S);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    if nanos == 0 {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hh, mm, ss)
    } else {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z", y, m, d, hh, mm, ss, nanos)
    }
}

/// Parses `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM:SS[.fraction]Z` (or `+00:00`), and
/// X's `created_at` form `YYYY-MM-DDTHH:MM:SS.mmmZ`; UTC only.
pub fn parse_iso(s: &str) -> Result<Time, String> {
    let s = s.trim();
    let bad = || format!("`{}` is not an ISO-8601 UTC time", s);
    let (date, rest) = match s.find('T') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut dp = date.split('-');
    let y: i64 = dp.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let m: u32 = dp.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let d: u32 = dp.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    if dp.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    let mut t = days_from_civil(y, m, d) * 86_400 * NS_PER_S;
    if let Some(rest) = rest {
        let rest = rest.strip_suffix('Z').or_else(|| rest.strip_suffix("+00:00")).ok_or_else(bad)?;
        let (clock, frac) = match rest.find('.') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, ""),
        };
        let mut cp = clock.split(':');
        let hh: i64 = cp.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
        let mm: i64 = cp.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
        let ss: i64 = cp.next().unwrap_or("0").parse().map_err(|_| bad())?;
        if cp.next().is_some() || hh > 23 || mm > 59 || ss > 60 {
            return Err(bad());
        }
        t += (hh * 3600 + mm * 60 + ss) * NS_PER_S;
        if !frac.is_empty() {
            if frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            let n: i64 = frac.parse().map_err(|_| bad())?;
            t += n * 10i64.pow(9 - frac.len() as u32);
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_posts_decode_to_their_creation_times() {
        // docs/design/fixtures/seed-posts.tsv, first and last rows.
        assert_eq!(iso(snowflake_created(1922715145840603213)), "2025-05-14T18:06:27.745000000Z");
        assert_eq!(&iso(snowflake_created(2111481108303134757))[..19], "2026-10-17T15:35:00");
    }

    #[test]
    fn iso_round_trips() {
        for s in ["1970-01-01T00:00:00Z", "2026-09-18T19:12:36Z", "2026-09-18T19:12:36.123456789Z", "2000-02-29T23:59:59Z"] {
            assert_eq!(iso(parse_iso(s).unwrap()), s);
        }
        assert_eq!(parse_iso("2026-09-18").unwrap(), parse_iso("2026-09-18T00:00:00Z").unwrap());
        assert_eq!(parse_iso("2026-09-18T19:12:36.123Z").unwrap(), parse_iso("2026-09-18T19:12:36.123000000Z").unwrap());
        assert!(parse_iso("2026-13-01").is_err());
        assert!(parse_iso("2026-09-18T19:12:36+02:00").is_err());
    }
}
