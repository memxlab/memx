use crate::db::SearchOptions;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

pub const DEFAULT_EMBEDDING_BASE_URL: &str = "https://api.deepinfra.com/v1/openai";
pub const DEFAULT_EMBEDDING_MODEL: &str = "Qwen/Qwen3-Embedding-0.6B";
pub const DEFAULT_EMBEDDING_DIMENSION: usize = 1024;
const DEFAULT_SERVER_HOST: &str = "127.0.0.1";
const DEFAULT_SERVER_PORT: u16 = 7878;

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

#[derive(Debug, Clone, Default)]
pub struct EmbeddingDraft {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct WriteConfigResult {
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

// ── TOML file structs (all fields optional) ────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct ConfigFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    database: Option<DbFileConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedding: Option<EmbeddingFileConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<ServerFileConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search: Option<SearchFileConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct DbFileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct EmbeddingFileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimension: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct ServerFileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct SearchFileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recency_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    importance_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    no_keyword_min_vector_score: Option<f64>,
}

// ── Config loading ─────────────────────────────────────────────────────────

impl Config {
    pub fn memx_dir_path() -> Result<PathBuf> {
        if let Ok(dir) = env::var("MEMX_HOME") {
            let dir = dir.trim();
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir));
            }
        }

        let home = dirs::home_dir().context("Cannot determine home directory")?;
        Ok(home.join(".memx"))
    }

    /// Returns the `~/.memx` directory, creating it on first call.
    pub fn memx_dir() -> Result<PathBuf> {
        let dir = Self::memx_dir_path()?;
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Cannot create directory: {}", dir.display()))?;
        }
        Ok(dir)
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::memx_dir_path()?.join("config.toml"))
    }

    pub fn load_embedding_draft() -> Result<EmbeddingDraft> {
        let file = Self::read_config_file()?.unwrap_or_default();
        let embedding = file.embedding.unwrap_or_default();

        Ok(EmbeddingDraft {
            api_key: embedding.api_key.filter(|value| !value.trim().is_empty()),
            base_url: embedding.base_url.filter(|value| !value.trim().is_empty()),
            model: embedding.model.filter(|value| !value.trim().is_empty()),
            dimension: embedding.dimension,
        })
    }

    pub fn write_embedding_config(embedding: &EmbeddingConfig) -> Result<WriteConfigResult> {
        let config_path = Self::memx_dir()?.join("config.toml");
        let backup_path = if config_path.exists() {
            let backup_path = config_path.with_extension("toml.bak");
            fs::copy(&config_path, &backup_path).with_context(|| {
                format!(
                    "Cannot back up {} to {}",
                    config_path.display(),
                    backup_path.display()
                )
            })?;
            Some(backup_path)
        } else {
            None
        };

        let file = Self::read_config_file()?.unwrap_or_default();
        let file = merge_embedding_config(file, embedding);
        let raw = toml::to_string_pretty(&file).context("Cannot serialize config")?;

        fs::write(&config_path, raw)
            .with_context(|| format!("Cannot write {}", config_path.display()))?;
        set_config_permissions(&config_path)?;

        Ok(WriteConfigResult {
            path: config_path,
            backup_path,
        })
    }

    /// Load config from `~/.memx/config.toml` with env-var overrides.
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();

        let memx_dir = Self::memx_dir()?;
        let config_path = Self::config_path()?;

        let file: ConfigFile = if config_path.exists() {
            Self::read_config_file()?.unwrap_or_default()
        } else if env::var("EMBEDDING_API_KEY").is_ok() {
            // Dev mode: no config file but env vars are set (e.g. via .env)
            ConfigFile::default()
        } else {
            anyhow::bail!(
                "Config file not found at {}.\nRun `memx setup` to create it.",
                config_path.display()
            );
        };
        resolve_config(file, &memx_dir)
    }

    fn read_config_file() -> Result<Option<ConfigFile>> {
        let config_path = Self::config_path()?;
        if !config_path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("Cannot read {}", config_path.display()))?;
        let file = toml::from_str(&raw)
            .with_context(|| format!("Invalid TOML in {}", config_path.display()))?;
        Ok(Some(file))
    }
}

fn resolve_config(file: ConfigFile, memx_dir: &std::path::Path) -> Result<Config> {
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
            DEFAULT_EMBEDDING_BASE_URL,
        ),
        model: env_or("EMBEDDING_MODEL", embed_file.model, DEFAULT_EMBEDDING_MODEL),
        dimension: env_or_usize(
            "EMBEDDING_DIMENSION",
            embed_file.dimension,
            DEFAULT_EMBEDDING_DIMENSION,
        )?,
    };

    let server = ServerConfig {
        host: env_or("SERVER_HOST", server_file.host, DEFAULT_SERVER_HOST),
        port: env_or_u16("SERVER_PORT", server_file.port, DEFAULT_SERVER_PORT)?,
    };

    let search = SearchConfig {
        semantic_weight: env_or_f64("SEARCH_SEMANTIC_WEIGHT", search_file.semantic_weight, 0.45)?,
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

fn merge_embedding_config(mut file: ConfigFile, embedding: &EmbeddingConfig) -> ConfigFile {
    file.embedding = Some(EmbeddingFileConfig {
        api_key: Some(embedding.api_key.clone()),
        base_url: Some(embedding.base_url.clone()),
        model: Some(embedding.model.clone()),
        dimension: Some(embedding.dimension),
    });

    match file.server.as_mut() {
        Some(server) => {
            if server.host.is_none() {
                server.host = Some(DEFAULT_SERVER_HOST.to_string());
            }
            if server.port.is_none() {
                server.port = Some(DEFAULT_SERVER_PORT);
            }
        }
        None => {
            file.server = Some(ServerFileConfig {
                host: Some(DEFAULT_SERVER_HOST.to_string()),
                port: Some(DEFAULT_SERVER_PORT),
            });
        }
    }

    file
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

fn set_config_permissions(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions)
            .with_context(|| format!("Cannot set permissions on {}", path.display()))?;
    }

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{
        merge_embedding_config, resolve_config, ConfigFile, EmbeddingConfig, EmbeddingFileConfig,
        SearchConfig, SearchFileConfig, ServerFileConfig, DEFAULT_EMBEDDING_BASE_URL,
        DEFAULT_EMBEDDING_DIMENSION, DEFAULT_EMBEDDING_MODEL,
    };
    use std::{path::Path, path::PathBuf};

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

    #[test]
    fn merge_embedding_config_preserves_other_sections() {
        let file = ConfigFile {
            embedding: Some(EmbeddingFileConfig {
                api_key: Some("old-key".to_string()),
                base_url: Some("https://old.example/v1".to_string()),
                model: Some("old-model".to_string()),
                dimension: Some(768),
            }),
            server: Some(ServerFileConfig {
                host: Some("0.0.0.0".to_string()),
                port: Some(9000),
            }),
            search: Some(SearchFileConfig {
                semantic_weight: Some(0.7),
                ..Default::default()
            }),
            ..Default::default()
        };

        let merged = merge_embedding_config(
            file,
            &EmbeddingConfig {
                api_key: "new-key".to_string(),
                base_url: "https://new.example/v1".to_string(),
                model: "new-model".to_string(),
                dimension: 1024,
            },
        );

        let embedding = merged.embedding.unwrap();
        assert_eq!(embedding.api_key.as_deref(), Some("new-key"));
        assert_eq!(
            embedding.base_url.as_deref(),
            Some("https://new.example/v1")
        );
        assert_eq!(embedding.model.as_deref(), Some("new-model"));
        assert_eq!(embedding.dimension, Some(1024));

        let server = merged.server.unwrap();
        assert_eq!(server.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(server.port, Some(9000));
        assert_eq!(merged.search.unwrap().semantic_weight, Some(0.7));
    }

    #[test]
    fn resolve_config_uses_defaults_for_missing_optional_fields() {
        let config = resolve_config(
            ConfigFile {
                embedding: Some(EmbeddingFileConfig {
                    api_key: Some("local".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Path::new("/tmp/memx-config-tests"),
        )
        .unwrap();

        assert_eq!(config.embedding.api_key, "local");
        assert_eq!(config.embedding.base_url, DEFAULT_EMBEDDING_BASE_URL);
        assert_eq!(config.embedding.model, DEFAULT_EMBEDDING_MODEL);
        assert_eq!(config.embedding.dimension, DEFAULT_EMBEDDING_DIMENSION);
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 7878);
        assert_eq!(config.database.path, "/tmp/memx-config-tests/memory.db");
    }

    #[test]
    fn memx_dir_path_prefers_memx_home_env() {
        let expected = if cfg!(windows) {
            PathBuf::from(r"C:\MemXData")
        } else {
            PathBuf::from("/tmp/memx-home")
        };
        std::env::set_var("MEMX_HOME", &expected);

        let resolved = super::Config::memx_dir_path().unwrap();

        std::env::remove_var("MEMX_HOME");

        assert_eq!(resolved, expected);
    }
}
