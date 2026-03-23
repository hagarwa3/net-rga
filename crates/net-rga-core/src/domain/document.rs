use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub String);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentLocator {
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub id: DocumentId,
    pub locator: DocumentLocator,
    pub extension: Option<String>,
    pub content_type: Option<String>,
    pub version: Option<String>,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}
