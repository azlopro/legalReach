// Enhanced src/database/mod.rs with new migrations
pub mod models;
pub mod repository;

use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;
use crate::{config::Settings, errors::Result};

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
    
    // Create leads table with enhanced status including 'disputed'
    let create_leads_table = "
        CREATE TABLE IF NOT EXISTS leads (
            id SERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            email VARCHAR(255) UNIQUE NOT NULL,
            status VARCHAR(50) NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'contacted', 'qualified', 'converted', 'lost', 'disputed')),
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
        CREATE INDEX IF NOT EXISTS idx_leads_email_domain ON leads((split_part(email, '@', 2)));
        
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
    
    client.batch_execute(create_leads_table).await?;

    // Create lead_conflicts table
    let create_conflicts_table = "
        CREATE TABLE IF NOT EXISTS lead_conflicts (
            id SERIAL PRIMARY KEY,
            lead_id INTEGER NOT NULL REFERENCES leads(id) ON DELETE CASCADE,
            conflict_type VARCHAR(50) NOT NULL CHECK (conflict_type IN ('same_domain', 'duplicate_email', 'similar_name')),
            conflict_details TEXT NOT NULL,
            conflicting_lead_id INTEGER REFERENCES leads(id) ON DELETE SET NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            resolved_at TIMESTAMPTZ,
            resolution_action VARCHAR(50) CHECK (resolution_action IN ('accept', 'discard', 'mark_contacted'))
        );
        
        CREATE INDEX IF NOT EXISTS idx_conflicts_lead_id ON lead_conflicts(lead_id);
        CREATE INDEX IF NOT EXISTS idx_conflicts_type ON lead_conflicts(conflict_type);
        CREATE INDEX IF NOT EXISTS idx_conflicts_resolved ON lead_conflicts(resolved_at);
        CREATE INDEX IF NOT EXISTS idx_conflicts_conflicting_lead ON lead_conflicts(conflicting_lead_id);
    ";
    
    client.batch_execute(create_conflicts_table).await?;

    // Create lead_validations table
    let create_validations_table = "
        CREATE TABLE IF NOT EXISTS lead_validations (
            id SERIAL PRIMARY KEY,
            lead_id INTEGER NOT NULL REFERENCES leads(id) ON DELETE CASCADE,
            validation_type VARCHAR(50) NOT NULL CHECK (validation_type IN ('email_active', 'email_syntax', 'domain_exists')),
            result VARCHAR(20) NOT NULL CHECK (result IN ('valid', 'invalid', 'unknown', 'pending')),
            details TEXT,
            validated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        
        CREATE INDEX IF NOT EXISTS idx_validations_lead_id ON lead_validations(lead_id);
        CREATE INDEX IF NOT EXISTS idx_validations_type ON lead_validations(validation_type);
        CREATE INDEX IF NOT EXISTS idx_validations_result ON lead_validations(result);
        CREATE INDEX IF NOT EXISTS idx_validations_validated_at ON lead_validations(validated_at);
        
        -- Unique constraint to prevent duplicate validations of the same type for the same lead within 24 hours
        CREATE UNIQUE INDEX IF NOT EXISTS idx_validations_unique_recent 
        ON lead_validations(lead_id, validation_type, DATE(validated_at));
    ";
    
    client.batch_execute(create_validations_table).await?;

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

    // Create system settings table for configuration
    let create_settings_table = "
        CREATE TABLE IF NOT EXISTS system_settings (
            id SERIAL PRIMARY KEY,
            setting_key VARCHAR(100) UNIQUE NOT NULL,
            setting_value JSONB NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        
        CREATE INDEX IF NOT EXISTS idx_settings_key ON system_settings(setting_key);
        
        -- Insert default conflict detection settings if not exists
        INSERT INTO system_settings (setting_key, setting_value) 
        VALUES ('conflict_detection', '{
            \"enable_same_domain_check\": true,
            \"enable_duplicate_email_check\": true,
            \"enable_similar_name_check\": false,
            \"same_domain_threshold\": 3,
            \"similar_name_threshold\": 0.8
        }'::jsonb)
        ON CONFLICT (setting_key) DO NOTHING;
        
        -- Insert default validation settings if not exists
        INSERT INTO system_settings (setting_key, setting_value) 
        VALUES ('validation_settings', '{
            \"enable_syntax_check\": true,
            \"enable_domain_check\": true,
            \"enable_external_validation\": false,
            \"timeout_seconds\": 10,
            \"batch_size\": 50
        }'::jsonb)
        ON CONFLICT (setting_key) DO NOTHING;
    ";
    
    client.batch_execute(create_settings_table).await?;

    // Create views for easier querying
    let create_views = "
        -- View for leads with conflict information
        CREATE OR REPLACE VIEW leads_with_conflicts AS
        SELECT 
            l.*,
            CASE 
                WHEN COUNT(c.id) > 0 THEN true 
                ELSE false 
            END as has_conflicts,
            COUNT(c.id) as conflict_count,
            STRING_AGG(DISTINCT c.conflict_type, ', ') as conflict_types
        FROM leads l
        LEFT JOIN lead_conflicts c ON l.id = c.lead_id AND c.resolved_at IS NULL
        GROUP BY l.id, l.name, l.email, l.status, l.created_at, l.updated_at, l.notes, l.source, l.phone, l.company;
        
        -- View for leads with validation information
        CREATE OR REPLACE VIEW leads_with_validations AS
        SELECT 
            l.*,
            v.result as latest_validation_result,
            v.validated_at as latest_validation_date,
            v.details as latest_validation_details
        FROM leads l
        LEFT JOIN LATERAL (
            SELECT result, validated_at, details
            FROM lead_validations 
            WHERE lead_id = l.id AND validation_type = 'email_active'
            ORDER BY validated_at DESC 
            LIMIT 1
        ) v ON true;
        
        -- View for dispute management dashboard
        CREATE OR REPLACE VIEW dispute_dashboard AS
        SELECT 
            l.id,
            l.name,
            l.email,
            l.status,
            l.created_at,
            COUNT(c.id) as total_conflicts,
            STRING_AGG(DISTINCT c.conflict_type, ', ') as conflict_types,
            STRING_AGG(DISTINCT c.conflict_details, ' | ') as conflict_details,
            MIN(c.created_at) as first_conflict_date,
            MAX(c.created_at) as latest_conflict_date
        FROM leads l
        JOIN lead_conflicts c ON l.id = c.lead_id AND c.resolved_at IS NULL
        WHERE l.status = 'disputed'
        GROUP BY l.id, l.name, l.email, l.status, l.created_at
        ORDER BY latest_conflict_date DESC;
    ";
    
    client.batch_execute(create_views).await?;

    // Create functions for advanced conflict detection
    let create_functions = "
        -- Function to extract domain from email
        CREATE OR REPLACE FUNCTION extract_email_domain(email TEXT)
        RETURNS TEXT AS $$
        BEGIN
            RETURN LOWER(split_part(email, '@', 2));
        END;
        $$ LANGUAGE plpgsql IMMUTABLE;
        
        -- Function to check for domain conflicts
        CREATE OR REPLACE FUNCTION check_domain_conflicts(domain_threshold INTEGER DEFAULT 3)
        RETURNS TABLE(domain TEXT, lead_count BIGINT, lead_ids INTEGER[]) AS $$
        BEGIN
            RETURN QUERY
            SELECT 
                extract_email_domain(l.email) as domain,
                COUNT(l.id) as lead_count,
                ARRAY_AGG(l.id ORDER BY l.created_at) as lead_ids
            FROM leads l
            WHERE l.status != 'disputed'
            GROUP BY extract_email_domain(l.email)
            HAVING COUNT(l.id) >= domain_threshold
            ORDER BY lead_count DESC;
        END;
        $$ LANGUAGE plpgsql;
        
        -- Function to auto-detect and create conflicts
        CREATE OR REPLACE FUNCTION auto_detect_conflicts()
        RETURNS INTEGER AS $$
        DECLARE
            conflict_count INTEGER := 0;
            domain_rec RECORD;
            lead_id_item INTEGER;
        BEGIN
            -- Detect same domain conflicts
            FOR domain_rec IN 
                SELECT * FROM check_domain_conflicts(3)
            LOOP
                -- Create conflicts for each lead in the domain (except the first one)
                FOR i IN 2..array_length(domain_rec.lead_ids, 1) LOOP
                    lead_id_item := domain_rec.lead_ids[i];
                    
                    -- Check if conflict already exists
                    IF NOT EXISTS (
                        SELECT 1 FROM lead_conflicts 
                        WHERE lead_id = lead_id_item 
                        AND conflict_type = 'same_domain' 
                        AND resolved_at IS NULL
                    ) THEN
                        INSERT INTO lead_conflicts (lead_id, conflict_type, conflict_details, conflicting_lead_id)
                        VALUES (
                            lead_id_item,
                            'same_domain',
                            format('Domain %s has %s leads', domain_rec.domain, domain_rec.lead_count),
                            domain_rec.lead_ids[1]
                        );
                        
                        -- Update lead status to disputed
                        UPDATE leads SET status = 'disputed' WHERE id = lead_id_item;
                        
                        conflict_count := conflict_count + 1;
                    END IF;
                END LOOP;
            END LOOP;
            
            RETURN conflict_count;
        END;
        $$ LANGUAGE plpgsql;
    ";
    
    client.batch_execute(create_functions).await?;
    
    tracing::info!("Database migrations completed successfully");
    tracing::info!("Created tables: leads, lead_conflicts, lead_validations, email_logs, system_settings");
    tracing::info!("Created views: leads_with_conflicts, leads_with_validations, dispute_dashboard");
    tracing::info!("Created functions: extract_email_domain, check_domain_conflicts, auto_detect_conflicts");
    
    Ok(())
}