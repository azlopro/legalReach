// Enhanced src/config.rs with validation and conflict detection settings
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
    pub conflict_detection: ConflictDetectionConfig,
    pub validation: ValidationConfig,
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
    pub enable_conflict_detection: bool,
    pub enable_email_validation: bool,
    pub auto_mark_disputed: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ZapierConfig {
    pub webhook_email: String,
    pub max_leads_per_batch: usize,
    pub min_interval_seconds: u64,
    pub max_interval_seconds: u64,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConflictDetectionConfig {
    pub enable_same_domain_check: bool,
    pub enable_duplicate_email_check: bool,
    pub enable_similar_name_check: bool,
    pub same_domain_threshold: i32,
    pub similar_name_threshold: f32,
    pub auto_analyze_on_import: bool,
    pub max_conflicts_per_lead: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ValidationConfig {
    pub enable_syntax_check: bool,
    pub enable_domain_check: bool,
    pub enable_external_validation: bool,
    pub external_api_key: Option<String>,
    pub external_api_url: Option<String>,
    pub timeout_seconds: u64,
    pub batch_size: usize,
    pub cache_results_hours: u64,
    pub retry_failed_validations: bool,
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

    /// Get conflict detection settings as model
    pub fn get_conflict_detection_settings(&self) -> crate::database::models::ConflictDetectionSettings {
        crate::database::models::ConflictDetectionSettings {
            enable_same_domain_check: self.conflict_detection.enable_same_domain_check,
            enable_duplicate_email_check: self.conflict_detection.enable_duplicate_email_check,
            enable_similar_name_check: self.conflict_detection.enable_similar_name_check,
            same_domain_threshold: self.conflict_detection.same_domain_threshold,
            similar_name_threshold: self.conflict_detection.similar_name_threshold,
        }
    }

    /// Get validation settings as service settings
    pub fn get_validation_settings(&self) -> crate::services::validation_service::ValidationSettings {
        crate::services::validation_service::ValidationSettings {
            enable_syntax_check: self.validation.enable_syntax_check,
            enable_domain_check: self.validation.enable_domain_check,
            enable_external_validation: self.validation.enable_external_validation,
            external_api_key: self.validation.external_api_key.clone(),
            external_api_url: self.validation.external_api_url.clone(),
            timeout_seconds: self.validation.timeout_seconds,
            batch_size: self.validation.batch_size,
        }
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
                enable_conflict_detection: true,
                enable_email_validation: true,
                auto_mark_disputed: true,
            },
            zapier: ZapierConfig {
                webhook_email: "odf86lbl@robot.zapier.com".to_string(),
                max_leads_per_batch: 500,
                min_interval_seconds: 1,
                max_interval_seconds: 300,
                enabled: true,
            },
            conflict_detection: ConflictDetectionConfig {
                enable_same_domain_check: true,
                enable_duplicate_email_check: true,
                enable_similar_name_check: false,
                same_domain_threshold: 3,
                similar_name_threshold: 0.8,
                auto_analyze_on_import: true,
                max_conflicts_per_lead: 10,
            },
            validation: ValidationConfig {
                enable_syntax_check: true,
                enable_domain_check: true,
                enable_external_validation: false,
                external_api_key: None,
                external_api_url: None,
                timeout_seconds: 10,
                batch_size: 50,
                cache_results_hours: 24,
                retry_failed_validations: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.server.port, 3000);
        assert!(settings.features.enable_conflict_detection);
        assert!(settings.features.enable_email_validation);
        assert_eq!(settings.conflict_detection.same_domain_threshold, 3);
        assert_eq!(settings.validation.batch_size, 50);
    }

    #[test]
    fn test_conflict_detection_settings_conversion() {
        let settings = Settings::default();
        let conflict_settings = settings.get_conflict_detection_settings();
        assert!(conflict_settings.enable_same_domain_check);
        assert!(conflict_settings.enable_duplicate_email_check);
        assert_eq!(conflict_settings.same_domain_threshold, 3);
    }

    #[test]
    fn test_validation_settings_conversion() {
        let settings = Settings::default();
        let validation_settings = settings.get_validation_settings();
        assert!(validation_settings.enable_syntax_check);
        assert!(validation_settings.enable_domain_check);
        assert_eq!(validation_settings.timeout_seconds, 10);
    }
}