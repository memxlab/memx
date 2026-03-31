use anyhow::{anyhow, bail, Context, Result};
use axum::Router;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    env, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};
use tower_http::cors::{Any, CorsLayer};

use crate::{
    config::{
        Config, EmbeddingConfig, EmbeddingDraft, WriteConfigResult, DEFAULT_EMBEDDING_BASE_URL,
        DEFAULT_EMBEDDING_DIMENSION, DEFAULT_EMBEDDING_MODEL,
    },
    db::{init_schema, DbPool, MemoryInput, MemoryType},
    embed::EmbedClient,
    mcp, os_service,
    routes::{link_routes, memory_routes, search_routes},
    service::{AppState, MemxService},
    updater::{self, UpdateOutcome},
};

const SETUP_PROBE_TEXT: &str = "memx setup verification";

#[derive(Parser)]
#[command(name = "memx", about = "MemX — AI memory server and CLI")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactively create or update the config
    Setup(SetupArgs),
    /// Check config and embedding connectivity
    Doctor,
    /// Show the MemX version
    Version,
    /// Update MemX to the latest published release
    Update,
    /// Manage the background MemX service
    Service(ServiceArgs),
    /// Remove the installed binary and local MemX data
    Uninstall(UninstallArgs),
    /// Start the HTTP server
    Serve,
    /// Start the MCP stdio server
    Mcp,
    /// Add a new memory
    Add {
        /// Memory content
        content: String,
        /// Memory type (semantic, episodic, procedural, emotional, reflective)
        #[arg(long, value_name = "TYPE")]
        r#type: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Importance score (0.0–1.0)
        #[arg(long)]
        importance: Option<f64>,
    },
    /// Search memories
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// List recent memories
    List {
        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: usize,
    },
}

#[derive(Args, Debug, Clone)]
struct SetupArgs {
    /// Provider preset used to prefill defaults
    #[arg(long, value_enum)]
    provider: Option<ProviderPreset>,
    /// Embedding API base URL
    #[arg(long)]
    base_url: Option<String>,
    /// Embedding model name
    #[arg(long)]
    model: Option<String>,
    /// Embedding API key
    #[arg(long)]
    api_key: Option<String>,
    /// Embedding vector dimension
    #[arg(long)]
    dimension: Option<usize>,
    /// Disable interactive prompts
    #[arg(long)]
    non_interactive: bool,
    /// Skip remote validation
    #[arg(long)]
    skip_validate: bool,
    /// Overwrite existing config without an extra prompt
    #[arg(long)]
    force: bool,
    /// Accept default confirmations
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug, Clone)]
struct UninstallArgs {
    /// Skip the confirmation prompt
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug, Clone)]
struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Subcommand, Debug, Clone)]
enum ServiceCommand {
    /// Install and start the background service
    Install,
    /// Start the background service
    Start,
    /// Stop the background service
    Stop,
    /// Show service status
    Status,
    /// Remove the background service
    Remove,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
enum ProviderPreset {
    Deepinfra,
    OpenaiCompatible,
    Custom,
}

impl ProviderPreset {
    fn label(self) -> &'static str {
        match self {
            Self::Deepinfra => "DeepInfra",
            Self::OpenaiCompatible => "OpenAI-compatible",
            Self::Custom => "Custom",
        }
    }

    fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Deepinfra => Some(DEFAULT_EMBEDDING_BASE_URL),
            Self::OpenaiCompatible | Self::Custom => None,
        }
    }

    fn default_model(self) -> Option<&'static str> {
        match self {
            Self::Deepinfra => Some(DEFAULT_EMBEDDING_MODEL),
            Self::OpenaiCompatible | Self::Custom => None,
        }
    }
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup(args) => cmd_setup(args).await,
        Commands::Doctor => cmd_doctor().await,
        Commands::Version => cmd_version(),
        Commands::Update => cmd_update().await,
        Commands::Service(args) => cmd_service(args),
        Commands::Uninstall(args) => cmd_uninstall(args),
        Commands::Serve => cmd_serve().await,
        Commands::Mcp => cmd_mcp().await,
        Commands::Add {
            content,
            r#type,
            tags,
            importance,
        } => cmd_add(content, r#type, tags, importance).await,
        Commands::Search { query, limit } => cmd_search(query, limit).await,
        Commands::List { limit, offset } => cmd_list(limit, offset).await,
    }
}

fn cmd_version() -> Result<()> {
    println!("{}", version_string());
    Ok(())
}

async fn cmd_update() -> Result<()> {
    let binary_path = env::current_exe().context("Cannot determine current executable path")?;
    ensure_installed_binary_path(&binary_path)?;

    let current_version = updater::current_version()?;
    match updater::update_current_binary(&binary_path, &current_version).await? {
        UpdateOutcome::UpToDate { current } => {
            println!("memx {current} is already up to date.");
        }
        UpdateOutcome::Updated {
            previous,
            latest,
            restarted_service,
        } => {
            println!("Updated MemX from {previous} to {latest}.");
            if restarted_service {
                println!("Background service restarted.");
            }
        }
        UpdateOutcome::Scheduled {
            previous,
            latest,
            restarted_service,
        } => {
            println!("Scheduled MemX update from {previous} to {latest}.");
            if restarted_service {
                println!("Background service will be started again after the update finishes.");
            }
            println!("Re-run `memx version` after this command exits to confirm the new version.");
        }
    }

    Ok(())
}

async fn init_service(config: &Config) -> Result<MemxService> {
    let db_pool = DbPool::new(&config.database.path).await?;
    {
        let conn = db_pool.get().await;
        init_schema(&conn, config.embedding.dimension).await?;
    }

    let embed_client = EmbedClient::new(
        config.embedding.api_key.clone(),
        config.embedding.base_url.clone(),
        config.embedding.model.clone(),
        config.embedding.dimension,
    );

    Ok(MemxService::new(
        db_pool,
        embed_client,
        config.search.to_options(),
    ))
}

// ── Subcommands ──────────────────────────────────────────────────────────────

async fn cmd_setup(args: SetupArgs) -> Result<()> {
    let config_path = Config::config_path()?;
    let existing = Config::load_embedding_draft()?;
    let has_existing = config_path.exists();

    if args.non_interactive && has_existing && !args.force {
        bail!(
            "Config already exists at {}.\nRe-run with --force to overwrite it.",
            config_path.display()
        );
    }

    let embedding = if args.non_interactive {
        build_non_interactive_embedding(&args, &existing).await?
    } else {
        build_interactive_embedding(&args, &existing, &config_path, has_existing).await?
    };

    println!();
    println!("Configuration summary:");
    println!("  base_url: {}", embedding.base_url);
    println!("  model: {}", embedding.model);
    println!("  api_key: {}", mask_secret(&embedding.api_key));
    println!("  dimension: {}", embedding.dimension);

    if !args.non_interactive && !args.force && !args.yes && !prompt_yes_no("Write config?", true)? {
        println!("Setup canceled.");
        return Ok(());
    }

    let result = Config::write_embedding_config(&embedding)?;
    print_setup_success(&result);

    if !args.non_interactive {
        let should_run_doctor = if args.yes {
            true
        } else {
            prompt_yes_no("Run `memx doctor` now?", true)?
        };

        if should_run_doctor {
            println!();
            cmd_doctor().await?;
        }
    }

    Ok(())
}

async fn cmd_doctor() -> Result<()> {
    let config_path = Config::config_path()?;

    if config_path.exists() {
        print_ok("Config file found");
    } else {
        print_fail("Config file found");
        println!("Reason: file not found");
        println!("Fix: run `memx setup`");
        bail!("Config file not found at {}", config_path.display());
    }

    let config = match Config::load() {
        Ok(config) => config,
        Err(err) => {
            print_fail("Configuration is valid");
            println!("Reason: {err}");
            println!("Fix: re-run `memx setup` or edit {}", config_path.display());
            return Err(err);
        }
    };

    print_ok("API key configured");
    print_ok("Embedding base URL configured");
    print_ok("Embedding model configured");

    let detected_dimension = match EmbedClient::detect_dimension(
        config.embedding.api_key.clone(),
        config.embedding.base_url.clone(),
        config.embedding.model.clone(),
        SETUP_PROBE_TEXT,
    )
    .await
    {
        Ok(dimension) => {
            print_ok("Embedding API reachable");
            dimension
        }
        Err(err) => {
            print_fail("Embedding API reachable");
            println!("Reason: {err}");
            println!("Fix: {}", doctor_fix_hint(&err.to_string(), &config_path));
            return Err(anyhow!(err));
        }
    };

    print_ok(&format!("Model returned {detected_dimension} dimensions"));

    if detected_dimension == config.embedding.dimension {
        print_ok("Config dimension matches response");
        return Ok(());
    }

    print_fail("Config dimension matches response");
    println!(
        "Reason: configured dimension {} does not match detected dimension {}",
        config.embedding.dimension, detected_dimension
    );
    println!(
        "Fix: update embedding.dimension in {}",
        config_path.display()
    );

    bail!(
        "Configured dimension {} does not match detected dimension {}",
        config.embedding.dimension,
        detected_dimension
    );
}

fn cmd_service(args: ServiceArgs) -> Result<()> {
    let binary_path = env::current_exe().context("Cannot determine current executable path")?;
    let memx_home = Config::memx_dir_path()?;

    match args.command {
        ServiceCommand::Install => {
            ensure_installed_binary_path(&binary_path)?;
            os_service::install(&binary_path, &memx_home)?;
            println!(
                "MemX background service installed and started with {}.",
                os_service::service_manager_name()
            );
        }
        ServiceCommand::Start => {
            os_service::start()?;
            println!("MemX background service started.");
        }
        ServiceCommand::Stop => {
            os_service::stop()?;
            println!("MemX background service stopped.");
        }
        ServiceCommand::Status => {
            let status = os_service::status()?;
            if status.trim().is_empty() {
                println!("Service status returned no output.");
            } else {
                println!("{status}");
            }
        }
        ServiceCommand::Remove => {
            os_service::remove()?;
            println!("MemX background service removed.");
        }
    }

    Ok(())
}

fn cmd_uninstall(args: UninstallArgs) -> Result<()> {
    let memx_dir = Config::memx_dir_path()?;
    let binary_path = env::current_exe().context("Cannot determine current executable path")?;
    let backup_binary_path = binary_backup_path(&binary_path);
    let manages_binary = is_managed_binary_path(&binary_path);

    println!("MemX uninstall");
    println!();
    println!("This will permanently delete:");
    println!("  - {}", memx_dir.display());
    println!("    This includes config.toml, memory.db, and any backups under ~/.memx.");

    if manages_binary {
        println!("  - {}", binary_path.display());
        if backup_binary_path.exists() {
            println!("  - {}", backup_binary_path.display());
        }
    } else {
        println!("  - local MemX data only");
        println!(
            "    The current executable looks like a development binary and will not be removed automatically."
        );
    }

    println!();
    println!("Your local MemX data will be lost.");

    if !args.yes && !prompt_yes_no("Continue uninstall?", false)? {
        println!("Uninstall canceled.");
        return Ok(());
    }

    let had_service = os_service::remove().is_ok();
    if had_service {
        println!("Removed background service configuration.");
    }

    if memx_dir.exists() {
        fs::remove_dir_all(&memx_dir)
            .with_context(|| format!("Failed to remove {}", memx_dir.display()))?;
        println!("Removed {}", memx_dir.display());
    } else {
        println!("Skipped {} (not found)", memx_dir.display());
    }

    match uninstall_current_binary(&binary_path)? {
        #[cfg(not(windows))]
        BinaryUninstallStatus::Removed => {
            println!("Removed {}", binary_path.display());
        }
        #[cfg(windows)]
        BinaryUninstallStatus::Scheduled => {
            println!(
                "Scheduled removal of {} after this process exits",
                binary_path.display()
            );
        }
        BinaryUninstallStatus::SkippedDevelopmentBinary => {
            println!(
                "Skipped removing {} because it looks like a Cargo build output",
                binary_path.display()
            );
        }
        BinaryUninstallStatus::NotFound => {
            println!("Skipped {} (not found)", binary_path.display());
        }
    }

    if manages_binary {
        if backup_binary_path.exists() {
            remove_file_if_exists(&backup_binary_path)?;
            println!("Removed {}", backup_binary_path.display());
        } else {
            println!("Skipped {} (not found)", backup_binary_path.display());
        }
    }

    println!();
    println!("MemX uninstall complete.");
    Ok(())
}

async fn cmd_serve() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memx=info,tower_http=debug".into()),
        )
        .init();

    let config = Config::load()?;
    tracing::info!("Configuration loaded");

    let memx = init_service(&config).await?;
    tracing::info!(
        "Database ready: {} | Embedding: {} ({}d)",
        config.database.path,
        config.embedding.model,
        config.embedding.dimension,
    );

    let state = AppState::new(memx.clone());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let rest_app = Router::new()
        .merge(memory_routes())
        .merge(search_routes())
        .merge(link_routes())
        .layer(cors);
    let app = Router::new().merge(rest_app).with_state(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Memory Server listening on {}", addr);
    tracing::info!("API endpoints:");
    tracing::info!("  POST   /memories           - Create memory");
    tracing::info!("  GET    /memories           - List memories");
    tracing::info!("  GET    /memories/{{id}}       - Get memory");
    tracing::info!("  PUT    /memories/{{id}}       - Update memory");
    tracing::info!("  DELETE /memories/{{id}}       - Delete memory");
    tracing::info!("  GET    /memories/search    - Search memories");
    tracing::info!("  POST   /memories/{{id}}/links - Create link");
    tracing::info!("  GET    /memories/{{id}}/links - Get links");
    tracing::info!("  DELETE /memories/links/{{id}} - Delete link");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn cmd_mcp() -> Result<()> {
    let config = Config::load()?;
    let service = init_service(&config).await?;
    mcp::serve_stdio(service).await
}

async fn cmd_add(
    content: String,
    memory_type: Option<String>,
    tags: Option<String>,
    importance: Option<f64>,
) -> Result<()> {
    let config = Config::load()?;
    let memx = init_service(&config).await?;

    let parsed_type = memory_type
        .as_deref()
        .map(|t| MemoryType::parse(t).ok_or_else(|| anyhow!("unknown memory type: {t}")))
        .transpose()?;

    let parsed_tags = tags.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

    let input = MemoryInput {
        content,
        memory_type: parsed_type,
        tags: parsed_tags,
        metadata: None,
        importance,
    };

    let memory = memx.add_memory(input).await?;
    let short_id = &memory.id[..8.min(memory.id.len())];
    println!("\u{2713} Memory created [{short_id}]");

    Ok(())
}

async fn cmd_search(query: String, limit: usize) -> Result<()> {
    let config = Config::load()?;
    let memx = init_service(&config).await?;

    let results = memx.search_memories(&query, limit).await?;

    if results.is_empty() {
        println!("No results found for \"{query}\"");
        return Ok(());
    }

    println!("Found {} results for \"{query}\":\n", results.len());
    for memory in &results {
        let short_id = &memory.id[..8.min(memory.id.len())];
        let score = memory.final_score.or(memory.score).unwrap_or(0.0);
        let date = format_timestamp(memory.created_at);
        println!("  [{short_id}] (score: {score:.2}) {date}");
        println!("  {}\n", memory.content);
    }

    Ok(())
}

async fn cmd_list(limit: usize, offset: usize) -> Result<()> {
    let config = Config::load()?;
    let memx = init_service(&config).await?;

    let memories = memx.list_memories(Some(limit), Some(offset)).await?;

    if memories.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    println!("Recent memories ({}):\n", memories.len());
    for memory in &memories {
        let short_id = &memory.id[..8.min(memory.id.len())];
        let mtype = memory.memory_type.as_str();
        let date = format_timestamp(memory.created_at);
        println!("  [{short_id}] {mtype:<11} {date}");
        println!("  {}\n", memory.content);
    }

    Ok(())
}

async fn build_non_interactive_embedding(
    args: &SetupArgs,
    existing: &EmbeddingDraft,
) -> Result<EmbeddingConfig> {
    let provider = args
        .provider
        .unwrap_or_else(|| infer_provider(existing.base_url.as_deref()));
    let base_url = resolve_text_value(
        args.base_url.as_ref(),
        existing.base_url.as_ref(),
        provider.default_base_url(),
        "base_url",
    )?;
    let model = resolve_text_value(
        args.model.as_ref(),
        existing.model.as_ref(),
        provider.default_model(),
        "model",
    )?;
    let api_key = resolve_text_value(
        args.api_key.as_ref(),
        existing.api_key.as_ref(),
        None,
        "api_key",
    )?;

    let dimension = resolve_dimension(
        &api_key,
        &base_url,
        &model,
        args.dimension.or(existing.dimension),
        args.skip_validate,
        false,
    )
    .await?;

    Ok(EmbeddingConfig {
        api_key,
        base_url,
        model,
        dimension,
    })
}

async fn build_interactive_embedding(
    args: &SetupArgs,
    existing: &EmbeddingDraft,
    config_path: &Path,
    has_existing: bool,
) -> Result<EmbeddingConfig> {
    println!("MemX setup");
    println!();
    println!("Config file: {}", config_path.display());
    if has_existing {
        println!(
            "Existing config found. A backup will be written to {}.",
            config_path.with_extension("toml.bak").display()
        );
    }

    let provider = match args.provider {
        Some(provider) => provider,
        None => prompt_provider(infer_provider(existing.base_url.as_deref()))?,
    };

    let model_default = first_non_empty(args.model.as_ref(), existing.model.as_ref())
        .map(ToOwned::to_owned)
        .or_else(|| provider.default_model().map(ToOwned::to_owned));
    let base_url_default = first_non_empty(args.base_url.as_ref(), existing.base_url.as_ref())
        .map(ToOwned::to_owned)
        .or_else(|| provider.default_base_url().map(ToOwned::to_owned));
    let api_key_default =
        first_non_empty(args.api_key.as_ref(), existing.api_key.as_ref()).map(ToOwned::to_owned);

    println!();
    let model = prompt_required_text("Embedding model", model_default.as_deref())?;
    let base_url = prompt_required_text("Embedding base URL", base_url_default.as_deref())?;
    let api_key = prompt_secret("Embedding API key", api_key_default.as_deref())?;

    let dimension = resolve_dimension(
        &api_key,
        &base_url,
        &model,
        args.dimension.or(existing.dimension),
        args.skip_validate,
        true,
    )
    .await?;

    Ok(EmbeddingConfig {
        api_key,
        base_url,
        model,
        dimension,
    })
}

async fn resolve_dimension(
    api_key: &str,
    base_url: &str,
    model: &str,
    configured_dimension: Option<usize>,
    skip_validate: bool,
    interactive: bool,
) -> Result<usize> {
    if skip_validate {
        return configured_dimension.context("--skip-validate requires --dimension");
    }

    println!();
    println!("Testing embedding API...");
    match EmbedClient::detect_dimension(
        api_key.to_string(),
        base_url.to_string(),
        model.to_string(),
        SETUP_PROBE_TEXT,
    )
    .await
    {
        Ok(detected) => {
            println!("Detected vector dimension: {detected}");
            if let Some(expected) = configured_dimension {
                if expected != detected {
                    bail!(
                        "Configured dimension {expected} does not match detected dimension {detected}"
                    );
                }
                return Ok(expected);
            }

            Ok(detected)
        }
        Err(err) if interactive => {
            eprintln!("Auto-detection failed: {err}");
            eprintln!("Enter the dimension manually to continue without remote validation.");
            prompt_dimension(
                "Embedding dimension",
                configured_dimension.unwrap_or(DEFAULT_EMBEDDING_DIMENSION),
            )
        }
        Err(err) => Err(err.into()),
    }
}

fn resolve_text_value(
    explicit: Option<&String>,
    existing: Option<&String>,
    provider_default: Option<&str>,
    field_name: &str,
) -> Result<String> {
    first_non_empty(explicit, existing)
        .map(ToOwned::to_owned)
        .or_else(|| provider_default.map(ToOwned::to_owned))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("Missing required field: {field_name}"))
}

fn infer_provider(base_url: Option<&str>) -> ProviderPreset {
    match base_url {
        Some(url) if url.trim_end_matches('/') == DEFAULT_EMBEDDING_BASE_URL => {
            ProviderPreset::Deepinfra
        }
        Some(_) => ProviderPreset::OpenaiCompatible,
        None => ProviderPreset::Deepinfra,
    }
}

fn first_non_empty<'a>(
    primary: Option<&'a String>,
    secondary: Option<&'a String>,
) -> Option<&'a str> {
    primary
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            secondary
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
        })
}

fn prompt_provider(default: ProviderPreset) -> Result<ProviderPreset> {
    loop {
        println!();
        println!("Select embedding provider:");
        println!("  1. {}", ProviderPreset::Deepinfra.label());
        println!("  2. {}", ProviderPreset::OpenaiCompatible.label());
        println!("  3. {}", ProviderPreset::Custom.label());

        let default_choice = match default {
            ProviderPreset::Deepinfra => "1",
            ProviderPreset::OpenaiCompatible => "2",
            ProviderPreset::Custom => "3",
        };

        match prompt_required_text("Select", Some(default_choice))?.as_str() {
            "1" => return Ok(ProviderPreset::Deepinfra),
            "2" => return Ok(ProviderPreset::OpenaiCompatible),
            "3" => return Ok(ProviderPreset::Custom),
            _ => println!("Please select 1, 2, or 3."),
        }
    }
}

fn prompt_required_text(label: &str, default: Option<&str>) -> Result<String> {
    loop {
        let value = prompt_text(label, default)?;
        if !value.trim().is_empty() {
            return Ok(value);
        }
        println!("{label} cannot be empty.");
    }
}

fn prompt_text(label: &str, default: Option<&str>) -> Result<String> {
    let mut stdout = io::stdout();
    match default {
        Some(default) if !default.is_empty() => write!(stdout, "{label} [{default}]: ")?,
        _ => write!(stdout, "{label}: ")?,
    }
    stdout.flush()?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .with_context(|| format!("Failed to read {label}"))?;
    let value = input.trim().to_string();

    if value.is_empty() {
        return Ok(default.unwrap_or_default().to_string());
    }

    Ok(value)
}

fn prompt_secret(label: &str, default: Option<&str>) -> Result<String> {
    loop {
        let prompt = if default.is_some() {
            format!("{label} [press Enter to keep current value]: ")
        } else {
            format!("{label}: ")
        };

        let value = rpassword::prompt_password(prompt)
            .with_context(|| format!("Failed to read {label}"))?;

        if !value.trim().is_empty() {
            return Ok(value);
        }

        if let Some(default) = default {
            if !default.trim().is_empty() {
                return Ok(default.to_string());
            }
        }

        println!("{label} cannot be empty.");
    }
}

fn prompt_dimension(label: &str, default: usize) -> Result<usize> {
    loop {
        let value = prompt_text(label, Some(&default.to_string()))?;
        match value.parse::<usize>() {
            Ok(dimension) if dimension > 0 => return Ok(dimension),
            _ => println!("Please enter a positive integer."),
        }
    }
}

fn prompt_yes_no(label: &str, default_yes: bool) -> Result<bool> {
    loop {
        let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
        let value = prompt_text(label, Some(suffix))?;
        let answer = if value == suffix {
            String::new()
        } else {
            value
        };

        if answer.trim().is_empty() {
            return Ok(default_yes);
        }

        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please answer y or n."),
        }
    }
}

fn print_setup_success(result: &WriteConfigResult) {
    println!();
    if let Some(backup_path) = &result.backup_path {
        println!("Backed up existing config to {}", backup_path.display());
    }
    println!("Config saved to {}", result.path.display());
}

fn print_ok(message: &str) {
    println!("[OK] {message}");
}

fn print_fail(message: &str) {
    println!("[FAIL] {message}");
}

fn mask_secret(secret: &str) -> String {
    if secret.is_empty() {
        return "<empty>".to_string();
    }

    if secret.chars().count() <= 8 {
        return "********".to_string();
    }

    let prefix: String = secret.chars().take(4).collect();
    let suffix: String = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    format!("{prefix}****{suffix}")
}

fn doctor_fix_hint(error: &str, config_path: &Path) -> String {
    if error.contains("401") || error.contains("403") {
        return format!("update embedding.api_key in {}", config_path.display());
    }

    if error.contains("404") || error.to_ascii_lowercase().contains("model") {
        return format!(
            "verify embedding.base_url and embedding.model in {}",
            config_path.display()
        );
    }

    if error.contains("Request failed") {
        return format!(
            "verify network connectivity and embedding.base_url in {}",
            config_path.display()
        );
    }

    format!("re-run `memx setup` or edit {}", config_path.display())
}

fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn version_string() -> String {
    format!("memx {}", env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BinaryUninstallStatus {
    #[cfg(not(windows))]
    Removed,
    #[cfg(windows)]
    Scheduled,
    SkippedDevelopmentBinary,
    NotFound,
}

fn uninstall_current_binary(binary_path: &Path) -> Result<BinaryUninstallStatus> {
    if !is_managed_binary_path(binary_path) {
        return Ok(BinaryUninstallStatus::SkippedDevelopmentBinary);
    }

    if !binary_path.exists() {
        return Ok(BinaryUninstallStatus::NotFound);
    }

    #[cfg(windows)]
    {
        schedule_windows_binary_removal(binary_path)?;
        Ok(BinaryUninstallStatus::Scheduled)
    }

    #[cfg(not(windows))]
    {
        remove_file_if_exists(binary_path)?;
        Ok(BinaryUninstallStatus::Removed)
    }
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn binary_backup_path(binary_path: &Path) -> PathBuf {
    let file_name = binary_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("memx");
    binary_path.with_file_name(format!("{file_name}.bak"))
}

fn is_managed_binary_path(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|value| value.to_str()),
        Some("memx") | Some("memx.exe")
    ) && !looks_like_cargo_target_binary(path)
}

fn ensure_installed_binary_path(binary_path: &Path) -> Result<()> {
    if looks_like_cargo_target_binary(binary_path) {
        bail!(
            "Refusing to manage an installed binary from {}.\nInstall MemX first, then re-run this command from the installed binary.",
            binary_path.display()
        );
    }

    Ok(())
}

fn looks_like_cargo_target_binary(path: &Path) -> bool {
    let mut saw_target = false;

    for component in path.components() {
        let Component::Normal(value) = component else {
            continue;
        };

        if value == "target" {
            saw_target = true;
            continue;
        }

        if saw_target && (value == "debug" || value == "release") {
            return true;
        }
    }

    false
}

#[cfg(windows)]
fn schedule_windows_binary_removal(binary_path: &Path) -> Result<()> {
    let command = format!(
        "ping 127.0.0.1 -n 3 >NUL & del /f /q \"{}\" >NUL 2>&1",
        binary_path.display()
    );

    std::process::Command::new("cmd")
        .args(["/C", &command])
        .spawn()
        .with_context(|| format!("Failed to schedule removal of {}", binary_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        binary_backup_path, is_managed_binary_path, looks_like_cargo_target_binary, version_string,
    };
    use std::path::Path;

    #[test]
    fn cargo_target_binary_is_not_treated_as_managed_install() {
        let path = Path::new("/tmp/memx-app/target/debug/memx");
        assert!(looks_like_cargo_target_binary(path));
        assert!(!is_managed_binary_path(path));
    }

    #[test]
    fn installed_binary_path_is_treated_as_managed_install() {
        let path = Path::new("/Users/demo/.local/bin/memx");
        assert!(!looks_like_cargo_target_binary(path));
        assert!(is_managed_binary_path(path));
    }

    #[test]
    fn backup_binary_path_appends_bak_suffix() {
        let path = Path::new("/Users/demo/.local/bin/memx");
        assert_eq!(
            binary_backup_path(path),
            Path::new("/Users/demo/.local/bin/memx.bak")
        );
    }

    #[test]
    fn version_string_matches_package_version() {
        assert_eq!(
            version_string(),
            format!("memx {}", env!("CARGO_PKG_VERSION"))
        );
    }
}
