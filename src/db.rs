use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Applies pending migrations embedded in the binary.
pub fn run_migrations(conn: &mut MysqlConnection, verbose: bool) -> anyhow::Result<()> {
    let applied = conn
        .run_pending_migrations(MIGRATIONS)
        .map_err(|e| anyhow::anyhow!("migration failed: {}", e))?;
    if verbose {
        for version in &applied {
            println!("Applied migration {}", version);
        }
        println!("Migrations are up to date.");
    }
    Ok(())
}

pub type DbPool = Pool<ConnectionManager<MysqlConnection>>;
pub type DbConn = diesel::r2d2::PooledConnection<ConnectionManager<MysqlConnection>>;

pub fn create_connection_pool(database_url: &str) -> DbPool {
    let manager = ConnectionManager::<MysqlConnection>::new(database_url);
    Pool::builder()
        .build(manager)
        .expect("Failed to create pool.")
}

#[cfg(test)]
pub fn create_connection() -> MysqlConnection {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is not set");
    MysqlConnection::establish(&database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", database_url))
}
