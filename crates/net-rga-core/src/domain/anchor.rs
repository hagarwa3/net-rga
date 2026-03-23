use std::fmt;

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

impl AnchorKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::LineSpan => "line_span",
            Self::PageSpan => "page_span",
            Self::SlideRegion => "slide_region",
            Self::SheetRange => "sheet_range",
            Self::ChunkSpan => "chunk_span",
            Self::ByteSpan => "byte_span",
            Self::TextSpan => "text_span",
        }
    }

    fn from_str(value: &str) -> Result<Self, AnchorParseError> {
        match value {
            "line_span" => Ok(Self::LineSpan),
            "page_span" => Ok(Self::PageSpan),
            "slide_region" => Ok(Self::SlideRegion),
            "sheet_range" => Ok(Self::SheetRange),
            "chunk_span" => Ok(Self::ChunkSpan),
            "byte_span" => Ok(Self::ByteSpan),
            "text_span" => Ok(Self::TextSpan),
            other => Err(AnchorParseError::InvalidKind(other.to_owned())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnchorParseError {
    InvalidFormat,
    InvalidKind(String),
    InvalidNumber(String),
}

impl fmt::Display for AnchorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid anchor reference format"),
            Self::InvalidKind(value) => write!(f, "invalid anchor kind: {value}"),
            Self::InvalidNumber(value) => write!(f, "invalid anchor number: {value}"),
        }
    }
}

impl std::error::Error for AnchorParseError {}

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

impl Anchor {
    pub fn stable_ref(&self) -> String {
        let mut parts = vec![format!("kind={}", self.kind.as_str())];
        push_part(&mut parts, "path", self.locator.path.as_deref());
        push_number(&mut parts, "page", self.locator.page);
        push_number(&mut parts, "slide", self.locator.slide);
        push_part(&mut parts, "sheet", self.locator.sheet.as_deref());
        push_part(&mut parts, "cell_range", self.locator.cell_range.as_deref());
        push_part(&mut parts, "chunk_id", self.locator.chunk_id.as_deref());
        push_number(&mut parts, "line_start", self.locator.line_start);
        push_number(&mut parts, "line_end", self.locator.line_end);
        push_number(&mut parts, "byte_start", self.locator.byte_start);
        push_number(&mut parts, "byte_end", self.locator.byte_end);
        push_number(&mut parts, "text_start", self.locator.text_start);
        push_number(&mut parts, "text_end", self.locator.text_end);
        parts.join("|")
    }

    pub fn from_stable_ref(value: &str) -> Result<Self, AnchorParseError> {
        if value.is_empty() {
            return Err(AnchorParseError::InvalidFormat);
        }

        let mut kind = None;
        let mut locator = AnchorLocator::default();

        for part in value.split('|') {
            let Some((key, raw_value)) = part.split_once('=') else {
                return Err(AnchorParseError::InvalidFormat);
            };
            let decoded = unescape_component(raw_value)?;
            match key {
                "kind" => kind = Some(AnchorKind::from_str(&decoded)?),
                "path" => locator.path = Some(decoded),
                "page" => locator.page = Some(parse_u32(&decoded)?),
                "slide" => locator.slide = Some(parse_u32(&decoded)?),
                "sheet" => locator.sheet = Some(decoded),
                "cell_range" => locator.cell_range = Some(decoded),
                "chunk_id" => locator.chunk_id = Some(decoded),
                "line_start" => locator.line_start = Some(parse_u32(&decoded)?),
                "line_end" => locator.line_end = Some(parse_u32(&decoded)?),
                "byte_start" => locator.byte_start = Some(parse_u64(&decoded)?),
                "byte_end" => locator.byte_end = Some(parse_u64(&decoded)?),
                "text_start" => locator.text_start = Some(parse_u64(&decoded)?),
                "text_end" => locator.text_end = Some(parse_u64(&decoded)?),
                _ => {}
            }
        }

        Ok(Self {
            kind: kind.ok_or(AnchorParseError::InvalidFormat)?,
            locator,
        })
    }
}

fn push_part(parts: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        parts.push(format!("{key}={}", escape_component(value)));
    }
}

fn push_number<T>(parts: &mut Vec<String>, key: &str, value: Option<T>)
where
    T: fmt::Display,
{
    if let Some(value) = value {
        parts.push(format!("{key}={value}"));
    }
}

fn escape_component(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace('=', "%3D")
}

fn unescape_component(value: &str) -> Result<String, AnchorParseError> {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            output.push(character);
            continue;
        }

        let first = chars.next().ok_or(AnchorParseError::InvalidFormat)?;
        let second = chars.next().ok_or(AnchorParseError::InvalidFormat)?;
        match (first, second) {
            ('2', '5') => output.push('%'),
            ('7', 'C') => output.push('|'),
            ('3', 'D') => output.push('='),
            _ => return Err(AnchorParseError::InvalidFormat),
        }
    }
    Ok(output)
}

fn parse_u32(value: &str) -> Result<u32, AnchorParseError> {
    value
        .parse::<u32>()
        .map_err(|_| AnchorParseError::InvalidNumber(value.to_owned()))
}

fn parse_u64(value: &str) -> Result<u64, AnchorParseError> {
    value
        .parse::<u64>()
        .map_err(|_| AnchorParseError::InvalidNumber(value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{Anchor, AnchorKind, AnchorLocator};

    #[test]
    fn anchor_stable_ref_round_trips_special_characters() {
        let anchor = Anchor {
            kind: AnchorKind::SheetRange,
            locator: AnchorLocator {
                path: Some("finance|ops=2026.xlsx".to_owned()),
                sheet: Some("Q1|North=West".to_owned()),
                cell_range: Some("A1:B2".to_owned()),
                ..AnchorLocator::default()
            },
        };

        let encoded = anchor.stable_ref();
        let decoded = Anchor::from_stable_ref(&encoded)
            .unwrap_or_else(|error| panic!("anchor ref should decode: {error}"));

        assert_eq!(decoded, anchor);
    }

    #[test]
    fn anchor_stable_ref_retains_numeric_locations() {
        let anchor = Anchor {
            kind: AnchorKind::LineSpan,
            locator: AnchorLocator {
                path: Some("docs/report.txt".to_owned()),
                line_start: Some(3),
                line_end: Some(4),
                byte_start: Some(15),
                byte_end: Some(42),
                ..AnchorLocator::default()
            },
        };

        let encoded = anchor.stable_ref();
        assert!(encoded.contains("line_start=3"));
        assert!(encoded.contains("byte_end=42"));
    }
}
