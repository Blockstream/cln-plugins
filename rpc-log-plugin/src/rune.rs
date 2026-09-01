use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE};

const OPERATOR_RUNE_TAG: &str = "operator#";
const RUNE_PREFIX_BYTES: usize = 32;

/// Searches for tag `operator#name` in the rune.
/// Returns error if failed to parse provided rune.
/// Returns None if no such tag.
pub(crate) fn rune_tag(rune: &str) -> Result<Option<String>> {
    // Rune uses RFC 4648 URL-safe Base64 alphabet
    let decoded = URL_SAFE.decode(rune).context("invalid rune")?;

    let metadata = decoded
        .get(RUNE_PREFIX_BYTES..)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .context("invalid rune metadata")?;

    Ok(metadata
        .split(['&', '|'])
        .find_map(|part| part.strip_prefix(OPERATOR_RUNE_TAG))
        .map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use crate::rune_tag;
    use anyhow::Result;

    #[test]
    fn test_parse_rune_operator() -> Result<()> {
        let example_rune = "_A7OO-xeVLnHX-zRLOhNGg3DDDMCvET1DZN-72WkbkVvcGVyYXRvciNPbGVn";
        let operator = rune_tag(example_rune)?;
        assert!(operator.is_some());
        assert_eq!(operator.unwrap(), "Oleg".to_owned());
        Ok(())
    }
}
