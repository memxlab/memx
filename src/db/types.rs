use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Memory {
    pub id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub memory_type: MemoryType,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub importance: f64,
    pub access_count: i64,
    pub last_accessed_at: Option<i64>,
    pub retrieval_count: i64,
    pub last_retrieved_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Episodic,
    Semantic,
    Procedural,
    Emotional,
    Reflective,
}

impl MemoryType {
    pub fn as_str(&self) -> &str {
        match self {
            MemoryType::Episodic => "episodic",
            MemoryType::Semantic => "semantic",
            MemoryType::Procedural => "procedural",
            MemoryType::Emotional => "emotional",
            MemoryType::Reflective => "reflective",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "episodic" => Some(MemoryType::Episodic),
            "semantic" => Some(MemoryType::Semantic),
            "procedural" => Some(MemoryType::Procedural),
            "emotional" => Some(MemoryType::Emotional),
            "reflective" => Some(MemoryType::Reflective),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInput {
    pub content: String,
    #[serde(rename = "type")]
    pub memory_type: Option<MemoryType>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub importance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLink {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub link_type: LinkType,
    pub strength: f64,
    pub bidirectional: bool,
    pub metadata: Option<serde_json::Value>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkType {
    Similar,
    Related,
    Contradicts,
    Extends,
    Supersedes,
    CausedBy,
    Temporal,
}

impl LinkType {
    pub fn as_str(&self) -> &str {
        match self {
            LinkType::Similar => "similar",
            LinkType::Related => "related",
            LinkType::Contradicts => "contradicts",
            LinkType::Extends => "extends",
            LinkType::Supersedes => "supersedes",
            LinkType::CausedBy => "caused_by",
            LinkType::Temporal => "temporal",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "similar" => Some(LinkType::Similar),
            "related" => Some(LinkType::Related),
            "contradicts" => Some(LinkType::Contradicts),
            "extends" => Some(LinkType::Extends),
            "supersedes" => Some(LinkType::Supersedes),
            "caused_by" => Some(LinkType::CausedBy),
            "temporal" => Some(LinkType::Temporal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInput {
    pub source_id: String,
    pub target_id: String,
    pub link_type: LinkType,
    pub strength: Option<f64>,
    pub bidirectional: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub semantic_weight: f64,
    pub recency_weight: f64,
    pub frequency_weight: f64,
    pub importance_weight: f64,
    pub decay_half_life_days: f64,
    pub no_keyword_min_vector_score: f64,
    pub enable_keyword: bool,
    pub enable_rejection: bool,
    pub enable_dedup: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            semantic_weight: 0.45,
            recency_weight: 0.25,
            frequency_weight: 0.05,
            importance_weight: 0.10,
            decay_half_life_days: 30.0,
            no_keyword_min_vector_score: 0.48,
            enable_keyword: true,
            enable_rejection: true,
            enable_dedup: true,
        }
    }
}
