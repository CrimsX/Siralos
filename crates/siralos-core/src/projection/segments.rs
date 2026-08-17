//! Context segment model, ordering, serialization, and fingerprinting.
//!
//! Three stability classes: stable < contextual < volatile. Ordering is
//! stability rank then TypeScript default string (UTF-16 code-unit)
//! comparison on the `id`. Fingerprint is SHA-256 of canonical JSON over
//! ordered stable `{id,title,content}` values only.

use crate::identity::{CanonicalValue, sha256_hex};

use super::estimator::estimate_tokens;

/// One stability class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stability {
    /// Rarely changes within a task/session.
    Stable,
    /// Task-specific but reasonably persistent.
    Contextual,
    /// Changes frequently.
    Volatile,
}

impl Stability {
    /// Rank for ordering (stable 0 < contextual 1 < volatile 2).
    pub fn rank(self) -> u8 {
        match self {
            Self::Stable => 0,
            Self::Contextual => 1,
            Self::Volatile => 2,
        }
    }

    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Contextual => "contextual",
            Self::Volatile => "volatile",
        }
    }

    /// Parse wire string.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "stable" => Some(Self::Stable),
            "contextual" => Some(Self::Contextual),
            "volatile" => Some(Self::Volatile),
            _ => None,
        }
    }
}

/// Input to the context projector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentInput {
    /// Stable id (ordering key).
    pub id: String,
    /// Stability class.
    pub stability: Stability,
    /// Human title (rendered as `[Title]`).
    pub title: String,
    /// Content.
    pub content: String,
}

/// One built segment with derived byte/token accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSegment {
    /// Segment id.
    pub id: String,
    /// Stability class.
    pub stability: Stability,
    /// Title.
    pub title: String,
    /// Content.
    pub content: String,
    /// UTF-8 bytes of `content` only.
    pub bytes: usize,
    /// `ceil(bytes / 4)` except empty → 0.
    pub estimated_tokens: usize,
}

/// Ordered projection result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextProjection {
    /// Stable segments in projector order.
    pub stable_segments: Vec<ContextSegment>,
    /// Contextual segments in projector order.
    pub contextual_segments: Vec<ContextSegment>,
    /// Volatile segments in projector order.
    pub volatile_segments: Vec<ContextSegment>,
    /// SHA-256 of canonical JSON over stable `[{id,title,content}]` only.
    pub stable_fingerprint: String,
    /// UTF-8 bytes of `serialize_segments(stableSegments)`.
    pub stable_bytes: usize,
    /// UTF-8 bytes of `serialize_segments(stable+contextual)`.
    pub stable_prefix_bytes: usize,
    /// Sum of content bytes for all three classes.
    pub total_bytes: usize,
    /// Sum of per-segment estimated tokens.
    pub estimated_tokens: usize,
}

/// Compare two Rust `&str` values as JavaScript default string comparison
/// (UTF-16 code-unit lexicographic order).
///
/// For strings in the Basic Multilingual Plane this coincides with Rust's
/// byte ordering. For supplementary scalars (surrogate pairs in JS) the
/// UTF-16 code-unit order can diverge from UTF-8 byte order, so we
/// compare via UTF-16 code units explicitly.
pub fn js_string_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let a_units: Vec<u16> = a.encode_utf16().collect();
    let b_units: Vec<u16> = b.encode_utf16().collect();
    a_units.cmp(&b_units)
}

/// Serialize one segment as `[Title]\\ncontent`.
pub fn serialize_segment(segment: &ContextSegment) -> String {
    format!("[{}]\n{}", segment.title, segment.content)
}

/// Serialize a slice of segments joined by `\\n\\n`.
pub fn serialize_segments(segments: &[ContextSegment]) -> String {
    segments.iter().map(serialize_segment).collect::<Vec<_>>().join("\n\n")
}

/// Build a `ContextSegment` from an input (copies, measures bytes/tokens).
fn build_segment(input: SegmentInput) -> ContextSegment {
    let bytes = input.content.len();
    let estimated_tokens = estimate_tokens(&input.content);
    ContextSegment {
        id: input.id,
        stability: input.stability,
        title: input.title,
        content: input.content,
        bytes,
        estimated_tokens,
    }
}

/// Project segments: copy, sort by (rank, js_string_cmp(id)), split by
/// class, compute byte/token accounting and stable fingerprint.
pub fn project_segments(inputs: Vec<SegmentInput>) -> ContextProjection {
    let mut sorted = inputs;
    sorted.sort_by(|a, b| {
        let rank = a.stability.rank().cmp(&b.stability.rank());
        if rank != std::cmp::Ordering::Equal {
            return rank;
        }
        js_string_cmp(&a.id, &b.id)
    });
    let built: Vec<ContextSegment> =
        sorted.into_iter().map(build_segment).collect();
    let stable_segments: Vec<ContextSegment> = built
        .iter()
        .filter(|s| s.stability == Stability::Stable)
        .cloned()
        .collect();
    let contextual_segments: Vec<ContextSegment> = built
        .iter()
        .filter(|s| s.stability == Stability::Contextual)
        .cloned()
        .collect();
    let volatile_segments: Vec<ContextSegment> = built
        .iter()
        .filter(|s| s.stability == Stability::Volatile)
        .cloned()
        .collect();

    // Stable fingerprint: SHA-256(canonical JSON([{id,title,content}] stable only)).
    let stable_values: Vec<CanonicalValue> = stable_segments
        .iter()
        .map(|seg| {
            let mut map = std::collections::BTreeMap::new();
            map.insert(
                "content".to_owned(),
                CanonicalValue::Str(seg.content.clone()),
            );
            map.insert("id".to_owned(), CanonicalValue::Str(seg.id.clone()));
            map.insert(
                "title".to_owned(),
                CanonicalValue::Str(seg.title.clone()),
            );
            CanonicalValue::Object(map)
        })
        .collect();
    let canonical = CanonicalValue::Array(stable_values).to_canonical();
    let stable_fingerprint = sha256_hex(canonical.as_bytes());

    let stable_bytes = serialize_segments(&stable_segments).len();
    let mut prefix_segments =
        Vec::with_capacity(stable_segments.len() + contextual_segments.len());
    prefix_segments.extend_from_slice(&stable_segments);
    prefix_segments.extend_from_slice(&contextual_segments);
    let stable_prefix_bytes = serialize_segments(&prefix_segments).len();
    let total_bytes = built.iter().map(|s| s.bytes).sum();
    let estimated_tokens = built.iter().map(|s| s.estimated_tokens).sum();

    ContextProjection {
        stable_segments,
        contextual_segments,
        volatile_segments,
        stable_fingerprint,
        stable_bytes,
        stable_prefix_bytes,
        total_bytes,
        estimated_tokens,
    }
}

/// Serialize the stable+contextual prefix for the provider `system` field.
pub fn serialize_prefix(projection: &ContextProjection) -> String {
    let mut segments = Vec::with_capacity(
        projection.stable_segments.len()
            + projection.contextual_segments.len(),
    );
    segments.extend_from_slice(&projection.stable_segments);
    segments.extend_from_slice(&projection.contextual_segments);
    serialize_segments(&segments)
}

#[cfg(test)]
mod tests {
    use super::{
        SegmentInput, Stability, js_string_cmp, project_segments,
        serialize_segments,
    };

    #[test]
    fn js_cmp_matches_rust_for_bmp() {
        assert_eq!(js_string_cmp("a", "b"), std::cmp::Ordering::Less);
        assert_eq!(js_string_cmp("b", "a"), std::cmp::Ordering::Greater);
        assert_eq!(js_string_cmp("a", "a"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn js_cmp_for_supplementary_scalar() {
        // U+10400 (DESERET CAPITAL LETTER LONG I) is outside BMP; its UTF-16
        // encoding is a surrogate pair 0xD801 0xDC00. Ensure js_string_cmp
        // handles supplementary scalars (no panic, deterministic).
        let a = "\u{10400}";
        let b = "\u{10401}";
        assert_eq!(js_string_cmp(a, b), std::cmp::Ordering::Less);
    }

    #[test]
    fn ordering_and_fingerprint() {
        let inputs = vec![
            SegmentInput {
                id: "v2".to_owned(),
                stability: Stability::Volatile,
                title: "Latest".to_owned(),
                content: "error".to_owned(),
            },
            SegmentInput {
                id: "s1".to_owned(),
                stability: Stability::Stable,
                title: "Instructions".to_owned(),
                content: "You are Siralos.".to_owned(),
            },
            SegmentInput {
                id: "c1".to_owned(),
                stability: Stability::Contextual,
                title: "Task".to_owned(),
                content: "Add health".to_owned(),
            },
        ];
        let proj = project_segments(inputs);
        assert_eq!(proj.stable_segments[0].id, "s1");
        assert_eq!(proj.contextual_segments[0].id, "c1");
        assert_eq!(proj.volatile_segments[0].id, "v2");
        assert_eq!(proj.stable_fingerprint.len(), 64);
        assert!(
            proj.stable_fingerprint.chars().all(|c| c.is_ascii_hexdigit())
        );
    }

    #[test]
    fn volatile_change_does_not_affect_stable_fingerprint() {
        let base = project_segments(vec![
            SegmentInput {
                id: "s1".to_owned(),
                stability: Stability::Stable,
                title: "Instructions".to_owned(),
                content: "You are Siralos.".to_owned(),
            },
            SegmentInput {
                id: "v1".to_owned(),
                stability: Stability::Volatile,
                title: "Latest".to_owned(),
                content: "error at line 4".to_owned(),
            },
        ]);
        let changed = project_segments(vec![
            SegmentInput {
                id: "s1".to_owned(),
                stability: Stability::Stable,
                title: "Instructions".to_owned(),
                content: "You are Siralos.".to_owned(),
            },
            SegmentInput {
                id: "v1".to_owned(),
                stability: Stability::Volatile,
                title: "Latest".to_owned(),
                content: "all clean".to_owned(),
            },
        ]);
        assert_eq!(changed.stable_fingerprint, base.stable_fingerprint);
        assert_eq!(changed.stable_bytes, base.stable_bytes);
        assert_eq!(changed.stable_prefix_bytes, base.stable_prefix_bytes);
    }

    #[test]
    fn serialization_format() {
        let proj = project_segments(vec![SegmentInput {
            id: "s1".to_owned(),
            stability: Stability::Stable,
            title: "T".to_owned(),
            content: "C".to_owned(),
        }]);
        let ser = serialize_segments(&proj.stable_segments);
        assert_eq!(ser, "[T]\nC");
    }
}
