//! Ordering version strings, where they can be ordered at all.

use std::cmp::Ordering;

/// Order two version strings numerically, or `None` where they cannot be
/// ordered -- pre-releases included, since managers spell them differently.
pub fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    let left = split_version(left);
    let right = split_version(right);

    for index in 0..left.len().max(right.len()) {
        let ordering = match (left.get(index), right.get(index)) {
            (Some(a), Some(b)) => match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                _ if a == b => Ordering::Equal,
                _ => return None,
            },
            // A missing segment counts as zero: `1.2` equals `1.2.0`.
            (Some(a), None) => match a.parse::<u64>() {
                Ok(0) => Ordering::Equal,
                Ok(_) => return Some(Ordering::Greater),
                Err(_) => return None,
            },
            (None, Some(b)) => match b.parse::<u64>() {
                Ok(0) => Ordering::Equal,
                Ok(_) => return Some(Ordering::Less),
                Err(_) => return None,
            },
            (None, None) => break,
        };
        if ordering != Ordering::Equal {
            return Some(ordering);
        }
    }

    Some(Ordering::Equal)
}

fn split_version(version: &str) -> Vec<&str> {
    version
        .split(['.', '-', '+', '_', ':', '~'])
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Some(Ordering::Greater));
        assert_eq!(compare_versions("0.15.0", "0.17.0"), Some(Ordering::Less));
    }

    #[test]
    fn a_missing_segment_counts_as_zero() {
        assert_eq!(compare_versions("1.2.1", "1.2"), Some(Ordering::Greater));
        assert_eq!(compare_versions("1.2", "1.2.0"), Some(Ordering::Equal));
    }

    #[test]
    fn an_unrankable_pair_is_admitted_rather_than_guessed() {
        assert_eq!(compare_versions("1.0.0-rc1", "1.0.0"), None);
        assert_eq!(compare_versions("stable", "1.0"), None);
    }
}
