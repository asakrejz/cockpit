//! Small texts for the commit list of the review panel.

/// First seven characters of a commit ID.
pub fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// `2024-05-13T09:30:00Z` → `13.05.2024 09:30` (UTC, as GitHub delivers it). Anything that does
/// not look like a date is returned unchanged.
pub fn commit_date(iso: &str) -> String {
    let (date, time) = iso.split_once('T').unwrap_or((iso, ""));
    let mut d = date.split('-');
    let (y, m, day) = (d.next(), d.next(), d.next());
    let (Some(y), Some(m), Some(day)) = (digits(y, 4), digits(m, 2), digits(day, 2)) else {
        return iso.to_string();
    };
    match time.get(..5) {
        Some(hm) => format!("{day}.{m}.{y} {hm}"),
        None => format!("{day}.{m}.{y}"),
    }
}

fn digits(s: Option<&str>, len: usize) -> Option<&str> {
    s.filter(|p| p.len() == len && p.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests;
