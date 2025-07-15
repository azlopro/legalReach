use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub email: EmailConfig,
    pub features: FeatureConfig,
    pub zapier: ZapierConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
    pub max_request_size: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database_name: String,
    pub max_connections: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub api_key: String,
    pub session_timeout_hours: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub from_email: String,
    pub from_name: String,
    pub mock_mode: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FeatureConfig {
    pub enable_email_sending: bool,
    pub max_leads_per_import: usize,
    pub max_leads_per_export: usize,
    pub enable_rate_limiting: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ZapierConfig {
    pub webhook_email: String,
    pub max_leads_per_batch: usize,
    pub min_interval_seconds: u64,
    pub max_interval_seconds: u64,
    pub enabled: bool,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let s = Config::builder()
            // Start with default configuration
            .add_source(File::with_name("config/default").required(false))
            // Add environment-specific configuration
            .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
            // Add local configuration (for development overrides)
            .add_source(File::with_name("config/local").required(false))
            // Add environment variables with prefix "LEGAL_REACH"
            .add_source(Environment::with_prefix("LEGAL_REACH").separator("__"))
            .build()?;

        s.try_deserialize()
    }

    pub fn database_url(&self) -> String {
        format!(
            "host={} port={} user={} password={} dbname={}",
            self.database.host,
            self.database.port,
            self.database.username,
            self.database.password,
            self.database.database_name
        )
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                cors_origins: vec!["http://localhost:3000".to_string()],
                max_request_size: 10 * 1024 * 1024, // 10MB
            },
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                username: "lead_manager_user".to_string(),
                password: "lead_manager_password".to_string(),
                database_name: "lead_management".to_string(),
                max_connections: 10,
            },
            auth: AuthConfig {
                api_key: "your-secure-api-key-here".to_string(),
                session_timeout_hours: 24,
            },
            email: EmailConfig {
                smtp_host: "smtp.gmail.com".to_string(),
                smtp_port: 587,
                smtp_username: "your-email@gmail.com".to_string(),
                smtp_password: "your-app-password".to_string(),
                from_email: "your-email@gmail.com".to_string(),
                from_name: "Lead Management System".to_string(),
                mock_mode: true,
            },
            features: FeatureConfig {
                enable_email_sending: true,
                max_leads_per_import: 1000,
                max_leads_per_export: 10000,
                enable_rate_limiting: true,
            },
            zapier: ZapierConfig {
                webhook_email: "odf86lbl@robot.zapier.com".to_string(),
                max_leads_per_batch: 500,
                min_interval_seconds: 1,
                max_interval_seconds: 300,
                enabled: true,
            },
        }
    }
}