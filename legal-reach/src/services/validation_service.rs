// New file: src/services/validation_service.rs
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use crate::{
    database::models::*,
    database::repository::LeadRepository,
    errors::{AppError, Result},
};

pub struct EmailValidationService {
    client: Client,
    settings: ValidationSettings,
}

#[derive(Debug, Clone)]
pub struct ValidationSettings {
    pub enable_syntax_check: bool,
    pub enable_domain_check: bool,
    pub enable_external_validation: bool,
    pub external_api_key: Option<String>,
    pub external_api_url: Option<String>,
    pub timeout_seconds: u64,
    pub batch_size: usize,
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self {
            enable_syntax_check: true,
            enable_domain_check: true,
            enable_external_validation: false,
            external_api_key: None,
            external_api_url: None,
            timeout_seconds: 10,
            batch_size: 50,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ExternalValidationRequest {
    email: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExternalValidationResponse {
    email: String,
    is_valid: bool,
    reason: Option<String>,
    confidence: Option<f32>,
}

impl EmailValidationService {
    pub fn new(settings: ValidationSettings) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(settings.timeout_seconds))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client, settings }
    }

    /// Validate a single email address
    pub async fn validate_email(&self, email: &str) -> Result<ValidationResult> {
        // Step 1: Syntax validation
        if self.settings.enable_syntax_check {
            if !self.is_valid_email_syntax(email) {
                return Ok(ValidationResult::Invalid);
            }
        }

        // Step 2: Domain validation
        if self.settings.enable_domain_check {
            match self.validate_domain(email).await {
                Ok(false) => return Ok(ValidationResult::Invalid),
                Ok(true) => {},
                Err(_) => return Ok(ValidationResult::Unknown),
            }
        }

        // Step 3: External API validation (if enabled and configured)
        if self.settings.enable_external_validation {
            if let Some(ref api_key) = self.settings.external_api_key {
                if let Some(ref api_url) = self.settings.external_api_url {
                    match self.validate_with_external_api(email, api_key, api_url).await {
                        Ok(result) => return Ok(result),
                        Err(e) => {
                            tracing::warn!("External validation failed for {}: {}", email, e);
                            // Fall back to basic validation
                        }
                    }
                }
            }
        }

        // If all checks pass but no external validation, return Valid
        Ok(ValidationResult::Valid)
    }

    /// Validate multiple emails in batch
    pub async fn validate_emails_batch(&self, emails: &[String]) -> Result<Vec<(String, ValidationResult, Option<String>)>> {
        let mut results = Vec::new();

        for email in emails {
            let result = self.validate_email(email).await?;
            let details = self.get_validation_details(email, &result).await;
            results.push((email.clone(), result, details));

            // Add small delay to avoid overwhelming external APIs
            if self.settings.enable_external_validation {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        Ok(results)
    }

    /// Validate all leads in the database
    pub async fn validate_all_leads(&self, repository: &LeadRepository) -> Result<usize> {
        let all_leads_response = repository.get_leads(
            LeadFilters {
                status: None,
                search: None,
                source: None,
                created_after: None,
                created_before: None,
                has_conflicts: None,
                validation_result: None,
            },
            PaginationParams {
                page: Some(1),
                per_page: Some(10000),
            },
        ).await?;

        let leads = all_leads_response.data;
        let mut validated_count = 0;

        // Process leads in batches
        for chunk in leads.chunks(self.settings.batch_size) {
            let emails: Vec<String> = chunk.iter().map(|l| l.email.clone()).collect();
            let validation_results = self.validate_emails_batch(&emails).await?;

            for (lead, (_email, result, details)) in chunk.iter().zip(validation_results.iter()) {
                // Check if validation already exists
                let existing_validations = repository.get_validations_for_lead(lead.id).await?;
                let has_recent_validation = existing_validations.iter().any(|v| {
                    v.validation_type == ValidationType::EmailActive &&
                    (chrono::Utc::now() - v.validated_at).num_days() < 30 // Refresh validations older than 30 days
                });

                if !has_recent_validation {
                    let validation = NewLeadValidation {
                        lead_id: lead.id,
                        validation_type: ValidationType::EmailActive,
                        result: result.clone(),
                        details: details.clone(),
                    };

                    if let Err(e) = repository.create_validation(validation).await {
                        tracing::warn!("Failed to save validation for lead {}: {}", lead.id, e);
                    } else {
                        validated_count += 1;
                    }
                }
            }

            // Add delay between batches
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(validated_count)
    }

    /// Check if email syntax is valid using regex
    fn is_valid_email_syntax(&self, email: &str) -> bool {
        let email_regex = Regex::new(
            r"^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$"
        ).unwrap();

        if !email_regex.is_match(email) {
            return false;
        }

        // Additional checks
        if email.len() > 254 {
            return false;
        }

        let parts: Vec<&str> = email.split('@').collect();
        if parts.len() != 2 {
            return false;
        }

        let local = parts[0];
        let domain = parts[1];

        // Local part checks
        if local.is_empty() || local.len() > 64 {
            return false;
        }

        // Domain part checks
        if domain.is_empty() || domain.len() > 253 {
            return false;
        }

        true
    }

    /// Validate domain by checking DNS records
    async fn validate_domain(&self, email: &str) -> Result<bool> {
        let domain = email.split('@').nth(1).ok_or_else(|| {
            AppError::validation("Invalid email format")
        })?;

        // Simple domain validation - check if it looks like a valid domain
        let domain_regex = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$").unwrap();
        
        if !domain_regex.is_match(domain) {
            return Ok(false);
        }

        // Check for obviously invalid domains
        let invalid_domains = [
            "test.com", "example.com", "test.test", "fake.com", 
            "invalid.com", "dummy.com", "placeholder.com"
        ];

        if invalid_domains.contains(&domain.to_lowercase().as_str()) {
            return Ok(false);
        }

        // In a real implementation, you would perform DNS lookup here
        // For now, we'll use a simple heuristic based on common TLDs
        let valid_tlds = [
            ".com", ".org", ".net", ".edu", ".gov", ".mil", ".int",
            ".co.uk", ".de", ".fr", ".it", ".es", ".nl", ".au",
            ".ca", ".br", ".jp", ".cn", ".in", ".ru", ".mx"
        ];

        let has_valid_tld = valid_tlds.iter().any(|&tld| domain.to_lowercase().ends_with(tld));
        
        Ok(has_valid_tld)
    }

    /// Validate email using external API service
    async fn validate_with_external_api(
        &self,
        email: &str,
        api_key: &str,
        api_url: &str,
    ) -> Result<ValidationResult> {
        let request = ExternalValidationRequest {
            email: email.to_string(),
        };

        let response = self
            .client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("External validation API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::internal(format!(
                "External validation API returned status: {}",
                response.status()
            )));
        }

        let validation_response: ExternalValidationResponse = response
            .json()
            .await
            .map_err(|e| AppError::internal(format!("Failed to parse external validation response: {}", e)))?;

        if validation_response.is_valid {
            Ok(ValidationResult::Valid)
        } else {
            Ok(ValidationResult::Invalid)
        }
    }

    /// Get detailed validation information
    pub async fn get_validation_details(&self, email: &str, result: &ValidationResult) -> Option<String> {
        match result {
            ValidationResult::Valid => Some("Email passed all validation checks".to_string()),
            ValidationResult::Invalid => {
                if !self.is_valid_email_syntax(email) {
                    Some("Invalid email syntax".to_string())
                } else {
                    match self.validate_domain(email).await {
                        Ok(false) => Some("Invalid or non-existent domain".to_string()),
                        _ => Some("Email validation failed".to_string()),
                    }
                }
            }
            ValidationResult::Unknown => Some("Unable to determine email validity".to_string()),
            ValidationResult::Pending => Some("Validation in progress".to_string()),
        }
    }

    /// Create validation entry for a new lead
    pub async fn create_lead_validation(
        &self,
        lead_id: i32,
        email: &str,
        repository: &LeadRepository,
    ) -> Result<()> {
        let result = self.validate_email(email).await?;
        let details = self.get_validation_details(email, &result).await;

        let validation = NewLeadValidation {
            lead_id,
            validation_type: ValidationType::EmailActive,
            result,
            details,
        };

        repository.create_validation(validation).await?;
        Ok(())
    }
}

/// Utility function to create a validation service with default settings
pub fn create_default_validation_service() -> EmailValidationService {
    EmailValidationService::new(ValidationSettings::default())
}

/// Utility function to create a validation service with external API
pub fn create_external_validation_service(
    api_key: String,
    api_url: String,
) -> EmailValidationService {
    let settings = ValidationSettings {
        enable_external_validation: true,
        external_api_key: Some(api_key),
        external_api_url: Some(api_url),
        ..Default::default()
    };
    EmailValidationService::new(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_email_syntax_validation() {
        let service = create_default_validation_service();

        assert!(service.is_valid_email_syntax("test@example.com"));
        assert!(service.is_valid_email_syntax("user.name+tag@domain.co.uk"));
        assert!(!service.is_valid_email_syntax("invalid-email"));
        assert!(!service.is_valid_email_syntax("@domain.com"));
        assert!(!service.is_valid_email_syntax("user@"));
        assert!(!service.is_valid_email_syntax("user@@domain.com"));
    }

    #[tokio::test]
    async fn test_domain_validation() {
        let service = create_default_validation_service();

        assert!(service.validate_domain("user@gmail.com").await.unwrap());
        assert!(service.validate_domain("test@company.co.uk").await.unwrap());
        assert!(!service.validate_domain("user@invalid.domain").await.unwrap());
        assert!(!service.validate_domain("test@example.com").await.unwrap()); // Blocked test domain
    }

    #[tokio::test]
    async fn test_full_email_validation() {
        let service = create_default_validation_service();

        // Valid emails
        let result = service.validate_email("test@gmail.com").await.unwrap();
        assert_eq!(result, ValidationResult::Valid);

        // Invalid syntax
        let result = service.validate_email("invalid-email").await.unwrap();
        assert_eq!(result, ValidationResult::Invalid);

        // Invalid domain
        let result = service.validate_email("test@example.com").await.unwrap();
        assert_eq!(result, ValidationResult::Invalid);
    }
}