pub fn fmt_mtime(mtime: Option<u32>) -> Option<String> {
    mtime.and_then(|t| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(i64::from(t), 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_mtime_some() {
        let s = fmt_mtime(Some(0));
        assert!(s.is_some());
    }

    #[test]
    fn test_fmt_mtime_none() {
        assert!(fmt_mtime(None).is_none());
    }
}
