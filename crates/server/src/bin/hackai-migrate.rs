#![forbid(unsafe_code)]

// Keep Cargo's dependency graph aware of the newest embedded SQLx migration.
const _: &str = include_str!("../../migrations/0035_shadow_bans.sql");

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let database_url = std::env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(())
}
