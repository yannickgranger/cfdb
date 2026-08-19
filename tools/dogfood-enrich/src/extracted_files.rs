use std::io;
use std::path::Path;
use std::process::Command;

pub fn file_paths_in_keyspace(
    cfdb_bin: &Path,
    db: &Path,
    keyspace: &str,
) -> io::Result<Vec<String>> {
    let output = Command::new(cfdb_bin)
        .arg("query")
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .arg("MATCH (f:File) RETURN f.path")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "cfdb query exited {}: {stderr}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_file_paths(&stdout).map_err(|reason| {
        io::Error::other(format!(
            "failed to parse cfdb query stdout: {reason}\nstdout: {stdout}"
        ))
    })
}

fn parse_file_paths(stdout: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("invalid JSON: {e}"))?;
    let rows = value
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "no `rows` array in JSON".to_string())?;
    if rows.is_empty() {
        return Err("empty `rows` array — keyspace has zero :File nodes".to_string());
    }
    let mut paths = Vec::with_capacity(rows.len());
    for (idx, row) in rows.iter().enumerate() {
        let path = row
            .get("f.path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("row {idx} missing string `f.path` column"))?;
        paths.push(path.to_string());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_sorts_and_dedups_paths() {
        let stdout = r#"{"rows":[
            {"f.path":"crates/b/src/lib.rs"},
            {"f.path":"crates/a/src/lib.rs"},
            {"f.path":"crates/b/src/lib.rs"}
        ],"warnings":[]}"#;
        assert_eq!(
            parse_file_paths(stdout).unwrap(),
            vec![
                "crates/a/src/lib.rs".to_string(),
                "crates/b/src/lib.rs".to_string()
            ]
        );
    }

    #[test]
    fn parse_rejects_empty_rows() {
        let err = parse_file_paths(r#"{"rows":[],"warnings":[]}"#).unwrap_err();
        assert!(err.contains("zero :File nodes"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_missing_path_column() {
        let err = parse_file_paths(r#"{"rows":[{"path":"x.rs"}]}"#).unwrap_err();
        assert!(err.contains("`f.path` column"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_invalid_json() {
        let err = parse_file_paths("not-json").unwrap_err();
        assert!(err.contains("invalid JSON"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_missing_rows_array() {
        let err = parse_file_paths(r#"{"warnings":[]}"#).unwrap_err();
        assert!(err.contains("`rows` array"), "unexpected error: {err}");
    }
}
