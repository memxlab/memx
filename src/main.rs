use anyhow::Result;
use axum::Router;
use memx::{
    config::Config,
    db::{init_schema, DbPool},
    embed::EmbedClient,
    routes::{link_routes, memory_routes, search_routes, AppState},
};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memx=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = Config::load()?;
    tracing::info!("Configuration loaded");

    // Initialize the database
    let db_pool = DbPool::new(&config.database.path).await?;
    tracing::info!("Database connection established: {}", config.database.path);

    // Initialize the database schema
    {
        let conn = db_pool.get().await;
        init_schema(&conn, config.embedding.dimension).await?;
        tracing::info!("Database schema initialized");
    }

    // Initialize the embedding client
    let embed_client = EmbedClient::new(
        config.embedding.api_key.clone(),
        config.embedding.base_url.clone(),
        config.embedding.model.clone(),
        config.embedding.dimension,
    );
    tracing::info!(
        "Embedding client initialized: {} ({}d)",
        config.embedding.model,
        config.embedding.dimension
    );

    // Create application state
    let state = AppState {
        db: db_pool,
        embed_client,
        search_options: config.search.to_options(),
    };

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build routes
    let app = Router::new()
        .merge(memory_routes())
        .merge(search_routes())
        .merge(link_routes())
        .layer(cors)
        .with_state(state);

    // Start the server
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
