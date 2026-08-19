use std::io;
use std::path::Path;
use std::process::Command;

pub fn count_items_in_keyspace(cfdb_bin: &Path, db: &Path, keyspace: &str) -> io::Result<usize> {
    count_items_with_kind(cfdb_bin, db, keyspace, None)
}

pub fn count_items_with_kind(
    cfdb_bin: &Path,
    db: &Path,
    keyspace: &str,
    kind: Option<&str>,
) -> io::Result<usize> {
    let cypher = match kind {
        Some(k) => format!("MATCH (i:Item) WHERE i.kind = \"{k}\" WITH count(i) AS n RETURN n"),
        None => "MATCH (i:Item) WITH count(i) AS n RETURN n".to_string(),
    };
    let output = Command::new(cfdb_bin)
        .arg("query")
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .arg(&cypher)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "cfdb query exited {}: {stderr}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_count(&stdout).map_err(|reason| {
        io::Error::other(format!(
            "failed to parse cfdb query stdout: {reason}\nstdout: {stdout}"
        ))
    })
}

pub fn count_deprecated_items_in_keyspace(
    cfdb_bin: &Path,
    db: &Path,
    keyspace: &str,
) -> io::Result<usize> {
    let cypher = "MATCH (i:Item) WHERE i.is_deprecated = true WITH count(i) AS n RETURN n";
    let output = Command::new(cfdb_bin)
        .arg("query")
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .arg(cypher)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "cfdb query exited {}: {stderr}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_count_or_zero(&stdout).map_err(|reason| {
        io::Error::other(format!(
            "failed to parse cfdb query stdout: {reason}\nstdout: {stdout}"
        ))
    })
}

fn parse_count_or_zero(stdout: &str) -> Result<usize, String> {
    let value: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("invalid JSON: {e}"))?;
    let rows = value
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "no `rows` array in JSON".to_string())?;
    let Some(first) = rows.first() else {
        return Ok(0);
    };
    let n = first
        .get("n")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "first row missing integer `n` column".to_string())?;
    Ok(n as usize)
}

fn parse_count(stdout: &str) -> Result<usize, String> {
    let value: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("invalid JSON: {e}"))?;
    let rows = value
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "no `rows` array in JSON".to_string())?;
    let first = rows
        .first()
        .ok_or_else(|| "empty `rows` array — keyspace has zero :Item nodes".to_string())?;
    let n = first
        .get("n")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "first row missing integer `n` column".to_string())?;
    Ok(n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_count_extracts_first_row_n() {
        let stdout = r#"{"rows":[{"n":1869}],"warnings":[]}"#;
        assert_eq!(parse_count(stdout).unwrap(), 1869);
    }

    #[test]
    fn parse_count_handles_pretty_printed_json() {
        let stdout = r#"{
  "rows": [
    {
      "n": 42
    }
  ]
}"#;
        assert_eq!(parse_count(stdout).unwrap(), 42);
    }

    #[test]
    fn parse_count_or_zero_maps_empty_rows_to_zero() {
        let stdout = r#"{"rows":[],"warnings":[]}"#;
        assert_eq!(parse_count_or_zero(stdout).unwrap(), 0);
    }

    #[test]
    fn parse_count_or_zero_extracts_first_row_n() {
        let stdout = r#"{"rows":[{"n":1}],"warnings":[]}"#;
        assert_eq!(parse_count_or_zero(stdout).unwrap(), 1);
    }

    #[test]
    fn parse_count_or_zero_still_rejects_missing_n() {
        let err = parse_count_or_zero(r#"{"rows":[{"total":3}]}"#).unwrap_err();
        assert!(err.contains("`n` column"), "unexpected error: {err}");
    }

    #[test]
    fn parse_count_rejects_empty_rows() {
        let stdout = r#"{"rows":[],"warnings":[]}"#;
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("zero :Item nodes"), "unexpected error: {err}");
    }

    #[test]
    fn parse_count_rejects_missing_n_column() {
        let stdout = r#"{"rows":[{"total":1869}]}"#;
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("`n` column"), "unexpected error: {err}");
    }

    #[test]
    fn parse_count_rejects_invalid_json() {
        let stdout = "not-json";
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("invalid JSON"), "unexpected error: {err}");
    }

    #[test]
    fn parse_count_rejects_missing_rows_array() {
        let stdout = r#"{"warnings":[]}"#;
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("`rows` array"), "unexpected error: {err}");
    }
}
