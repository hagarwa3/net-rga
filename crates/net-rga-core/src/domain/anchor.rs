use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    LineSpan,
    PageSpan,
    SlideRegion,
    SheetRange,
    ChunkSpan,
    ByteSpan,
    TextSpan,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorLocator {
    pub path: Option<String>,
    pub page: Option<u32>,
    pub slide: Option<u32>,
    pub sheet: Option<String>,
    pub cell_range: Option<String>,
    pub chunk_id: Option<String>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub byte_start: Option<u64>,
    pub byte_end: Option<u64>,
    pub text_start: Option<u64>,
    pub text_end: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub kind: AnchorKind,
    pub locator: AnchorLocator,
}

