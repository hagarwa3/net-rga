pub const MANIFEST_SCHEMA_V1: &str = include_str!("schema.sql");

#[cfg(test)]
mod tests {
    use super::MANIFEST_SCHEMA_V1;

    #[test]
    fn manifest_schema_includes_core_tables() {
        for table_name in [
            "corpora",
            "documents",
            "sync_checkpoints",
            "tombstones",
            "failure_records",
        ] {
            assert!(MANIFEST_SCHEMA_V1.contains(table_name));
        }
    }
}

