pub mod models;
pub mod repository;

use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;
use crate::{config::Settings, errors::Result};

pub use models::*;
pub use repository::*;

pub async fn create_pool(settings: &Settings) -> Result<Pool> {
    let mut cfg = Config::new();
    cfg.host = Some(settings.database.host.clone());
    cfg.port = Some(settings.database.port);
    cfg.user = Some(settings.database.username.clone());
    cfg.password = Some(settings.database.password.clone());
    cfg.dbname = Some(settings.database.database_name.clone());
    
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;
    
    // Run migrations
    run_migrations(&pool).await?;
    
    Ok(pool)
}

async fn run_migrations(pool: &Pool) -> Result<()> {
    let client = pool.get().await?;
    
    // Create leads table with status
    let create_table_query = "
        CREATE TABLE IF NOT EXISTS leads (
            id SERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            email VARCHAR(255) UNIQUE NOT NULL,
            status VARCHAR(50) NOT NULL DEFAULT 'new',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            notes TEXT,
            source VARCHAR(100),
            phone VARCHAR(50),
            company VARCHAR(255)
        );
        
        CREATE INDEX IF NOT EXISTS idx_leads_status ON leads(status);
        CREATE INDEX IF NOT EXISTS idx_leads_email ON leads(email);
        CREATE INDEX IF NOT EXISTS idx_leads_created_at ON leads(created_at);
        
        -- Create trigger to update updated_at timestamp
        CREATE OR REPLACE FUNCTION update_updated_at_column()
        RETURNS TRIGGER AS $$
        BEGIN
            NEW.updated_at = NOW();
            RETURN NEW;
        END;
        $$ language 'plpgsql';
        
        DROP TRIGGER IF EXISTS update_leads_updated_at ON leads;
        CREATE TRIGGER update_leads_updated_at
        BEFORE UPDATE ON leads
        FOR EACH ROW
        EXECUTE FUNCTION update_updated_at_column();
    ";
    
    client.batch_execute(create_table_query).await?;
    
    // Create email_logs table for tracking sent emails
    let create_email_logs_query = "
        CREATE TABLE IF NOT EXISTS email_logs (
            id SERIAL PRIMARY KEY,
            lead_id INTEGER NOT NULL REFERENCES leads(id) ON DELETE CASCADE,
            subject VARCHAR(500) NOT NULL,
            body TEXT NOT NULL,
            sent_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            status VARCHAR(50) NOT NULL DEFAULT 'sent'
        );
        
        CREATE INDEX IF NOT EXISTS idx_email_logs_lead_id ON email_logs(lead_id);
        CREATE INDEX IF NOT EXISTS idx_email_logs_sent_at ON email_logs(sent_at);
    ";
    
    client.batch_execute(create_email_logs_query).await?;
    
    tracing::info!("Database migrations completed successfully");
    Ok(())
}