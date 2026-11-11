//! Terminal output helpers. All server-provided text is sanitized before display.

use gitconflux_api::sanitize;

const MAX_CELL: usize = 60;

fn cell(s: &str) -> String {
    let clean = sanitize(s);
    if clean.chars().count() > MAX_CELL {
        let mut t: String = clean.chars().take(MAX_CELL - 1).collect();
        t.push('…');
        t
    } else {
        clean
    }
}

/// Renders an aligned, plain-text table.
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| cell(c)).collect())
        .collect();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in &cells {
        for (i, c) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(c.chars().count());
            }
        }
    }
    let line = |row: &[String]| {
        row.iter()
            .enumerate()
            .map(|(i, c)| format!("{c:<w$}", w = widths.get(i).copied().unwrap_or(0)))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_owned()
    };
    let head: Vec<String> = headers.iter().map(|h| (*h).to_owned()).collect();
    let mut out = vec![line(&head)];
    out.extend(cells.iter().map(|r| line(r)));
    out.join("\n")
}

/// Formats Unix milliseconds as RFC 3339 UTC (no external time dependency).
pub fn ts_millis(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_format() {
        assert_eq!(ts_millis(0), "1970-01-01T00:00:00Z");
        assert_eq!(ts_millis(1_709_164_800_000), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn table_aligns_and_sanitizes() {
        let t = table(&["A", "B"], &[vec!["x\x1b[0m".into(), "long value".into()]]);
        assert!(!t.contains('\x1b'));
        assert_eq!(t.lines().count(), 2);
    }

    #[test]
    fn long_cells_truncate() {
        assert!(table(&["A"], &[vec!["z".repeat(200)]]).contains('…'));
    }
}
