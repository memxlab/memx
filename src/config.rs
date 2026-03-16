use crate::db::SearchOptions;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

// ── Public config structs ──────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub embedding: EmbeddingConfig,
    pub server: ServerConfig,
    pub search: SearchConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub dimension: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub semantic_weight: f64,
    pub recency_weight: f64,
    pub frequency_weight: f64,
    pub importance_weight: f64,
    pub no_keyword_min_vector_score: f64,
}

impl SearchConfig {
    pub fn to_options(&self) -> SearchOptions {
        SearchOptions {
            semantic_weight: self.semantic_weight,
            recency_weight: self.recency_weight,
            frequency_weight: self.frequency_weight,
            importance_weight: self.importance_weight,
            decay_half_life_days: SearchOptions::default().decay_half_life_days,
            no_keyword_min_vector_score: self.no_keyword_min_vector_score,
            enable_keyword: true,
            enable_rejection: true,
            enable_dedup: true,
        }
    }
}

// ── TOML file structs (all fields optional) ────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    database: Option<DbFileConfig>,
    embedding: Option<EmbeddingFileConfig>,
    server: Option<ServerFileConfig>,
    search: Option<SearchFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct DbFileConfig {
    path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct EmbeddingFileConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    dimension: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct ServerFileConfig {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Deserialize, Default)]
struct SearchFileConfig {
    semantic_weight: Option<f64>,
    recency_weight: Option<f64>,
    frequency_weight: Option<f64>,
    importance_weight: Option<f64>,
    no_keyword_min_vector_score: Option<f64>,
}

// ── Config loading ─────────────────────────────────────────────────────────

impl Config {
    /// Returns the `~/.memx` directory, creating it on first call.
    pub fn memx_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        let dir = home.join(".memx");
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Cannot create directory: {}", dir.display()))?;
        }
        Ok(dir)
    }

    /// Load config from `~/.memx/config.toml` with env-var overrides.
    ///
    /// On first run (no config file), generates a template and exits with instructions.
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();

        let memx_dir = Self::memx_dir()?;
        let config_path = memx_dir.join("config.toml");

        let file: ConfigFile = if config_path.exists() {
            let raw = fs::read_to_string(&config_path)
                .with_context(|| format!("Cannot read {}", config_path.display()))?;
            toml::from_str(&raw)
                .with_context(|| format!("Invalid TOML in {}", config_path.display()))?
        } else if env::var("EMBEDDING_API_KEY").is_ok() {
            // Dev mode: no config file but env vars are set (e.g. via .env)
            ConfigFile::default()
        } else {
            fs::write(&config_path, config_template(&memx_dir))
                .with_context(|| format!("Cannot write {}", config_path.display()))?;
            eprintln!("✓ Created config file: {}", config_path.display());
            eprintln!("  Fill in your embedding API key, then restart MemX.");
            anyhow::bail!(
                "Config file created at {}. Please configure it before running.",
                config_path.display()
            );
        };

        let ConfigFile {
            database: db_file,
            embedding: embed_file,
            server: server_file,
            search: search_file,
        } = file;

        let db_file = db_file.unwrap_or_default();
        let embed_file = embed_file.unwrap_or_default();
        let server_file = server_file.unwrap_or_default();
        let search_file = search_file.unwrap_or_default();

        let default_db = memx_dir.join("memory.db").to_string_lossy().to_string();

        // embedding.api_key is the only truly required field
        let embedding_api_key = env::var("EMBEDDING_API_KEY")
            .ok()
            .or(embed_file.api_key)
            .filter(|s| !s.trim().is_empty())
            .context(
                "embedding.api_key is required.\n\
                 Set it in ~/.memx/config.toml or via the EMBEDDING_API_KEY env var.",
            )?;

        let database = DatabaseConfig {
            path: env_or("DATABASE_PATH", db_file.path, &default_db),
        };

        let embedding = EmbeddingConfig {
            api_key: embedding_api_key,
            base_url: env_or(
                "EMBEDDING_BASE_URL",
                embed_file.base_url,
                "https://api.deepinfra.com/v1/openai",
            ),
            model: env_or(
                "EMBEDDING_MODEL",
                embed_file.model,
                "Qwen/Qwen3-Embedding-0.6B",
            ),
            dimension: env_or_usize("EMBEDDING_DIMENSION", embed_file.dimension, 1024)?,
        };

        let server = ServerConfig {
            host: env_or("SERVER_HOST", server_file.host, "127.0.0.1"),
            port: env_or_u16("SERVER_PORT", server_file.port, 7878)?,
        };

        let search = SearchConfig {
            semantic_weight: env_or_f64(
                "SEARCH_SEMANTIC_WEIGHT",
                search_file.semantic_weight,
                0.45,
            )?,
            recency_weight: env_or_f64("SEARCH_RECENCY_WEIGHT", search_file.recency_weight, 0.25)?,
            frequency_weight: env_or_f64(
                "SEARCH_FREQUENCY_WEIGHT",
                search_file.frequency_weight,
                0.05,
            )?,
            importance_weight: env_or_f64(
                "SEARCH_IMPORTANCE_WEIGHT",
                search_file.importance_weight,
                0.10,
            )?,
            no_keyword_min_vector_score: env_or_f64(
                "SEARCH_NO_KEYWORD_MIN_VECTOR_SCORE",
                search_file.no_keyword_min_vector_score,
                0.48,
            )?,
        };

        Ok(Config {
            database,
            embedding,
            server,
            search,
        })
    }
}

// ── Config file template ───────────────────────────────────────────────────

fn config_template(memx_dir: &std::path::Path) -> String {
    let db_path = memx_dir.join("memory.db");
    format!(
        r#"# MemX Configuration
# Config:   ~/.memx/config.toml
# Database: {db_path}

[embedding]
# Your embedding API key (required)
api_key = ""
base_url = "https://api.deepinfra.com/v1/openai"
model = "Qwen/Qwen3-Embedding-0.6B"
dimension = 1024

[server]
host = "127.0.0.1"
port = 7878

# Uncomment to use a custom database path:
# [database]
# path = "{db_path}"
"#,
        db_path = db_path.display()
    )
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn env_or(env_key: &str, file_val: Option<String>, default: &str) -> String {
    env::var(env_key)
        .ok()
        .or(file_val)
        .unwrap_or_else(|| default.to_string())
}

fn env_or_usize(env_key: &str, file_val: Option<usize>, default: usize) -> Result<usize> {
    if let Ok(val) = env::var(env_key) {
        return val.parse().with_context(|| format!("Invalid {}", env_key));
    }
    Ok(file_val.unwrap_or(default))
}

fn env_or_u16(env_key: &str, file_val: Option<u16>, default: u16) -> Result<u16> {
    if let Ok(val) = env::var(env_key) {
        return val.parse().with_context(|| format!("Invalid {}", env_key));
    }
    Ok(file_val.unwrap_or(default))
}

fn env_or_f64(env_key: &str, file_val: Option<f64>, default: f64) -> Result<f64> {
    if let Ok(val) = env::var(env_key) {
        return val.parse().with_context(|| format!("Invalid {}", env_key));
    }
    Ok(file_val.unwrap_or(default))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::SearchConfig;

    #[test]
    fn converts_search_config_to_runtime_options() {
        let config = SearchConfig {
            semantic_weight: 0.4,
            recency_weight: 0.3,
            frequency_weight: 0.2,
            importance_weight: 0.1,
            no_keyword_min_vector_score: 0.55,
        };

        let options = config.to_options();

        assert_eq!(options.semantic_weight, 0.4);
        assert_eq!(options.recency_weight, 0.3);
        assert_eq!(options.frequency_weight, 0.2);
        assert_eq!(options.importance_weight, 0.1);
        assert_eq!(options.decay_half_life_days, 30.0);
        assert_eq!(options.no_keyword_min_vector_score, 0.55);
    }
}
