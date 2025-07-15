// Enhanced src/services/csv_service.rs with conflict detection
use bytes::Bytes;
use csv::{Reader, Writer};
use regex::Regex;
use std::io::Cursor;
use crate::{
    database::models::{Lead, NewLead, LeadWithDetails},
    database::repository::LeadRepository,
    services::{
        conflict_service::ConflictDetectionService,
        validation_service::EmailValidationService,
    },
    errors::{AppError, Result},
};

#[derive(Debug, Clone)]
pub struct CsvImportResult {
    pub imported_leads: Vec<Lead>,
    pub disputed_leads: Vec<Lead>,
    pub skipped_duplicates: Vec<String>,
    pub validation_errors: Vec<String>,
    pub import_summary: ImportSummary,
}

#[derive(Debug, Clone)]
pub struct ImportSummary {
    pub total_processed: usize,
    pub successfully_imported: usize,
    pub marked_disputed: usize,
    pub skipped_duplicates: usize,
    pub validation_failures: usize,
}

pub struct EnhancedCsvService {
    conflict_service: ConflictDetectionService,
    validation_service: EmailValidationService,
}

impl EnhancedCsvService {
    pub fn new(
        conflict_service: ConflictDetectionService,
        validation_service: EmailValidationService,
    ) -> Self {
        Self {
            conflict_service,
            validation_service,
        }
    }

    /// Parse CSV and import leads with conflict detection and validation
    pub async fn import_leads_with_conflict_detection(
        &self,
        data: &Bytes,
        repository: &LeadRepository,
        enable_validation: bool,
    ) -> Result<CsvImportResult> {
        // Parse CSV data
        let new_leads = self.parse_csv_data(data)?;
        
        tracing::info!("Parsed {} leads from CSV", new_leads.len());

        // Detect conflicts for the batch
        let batch_conflicts = self
            .conflict_service
            .detect_conflicts_batch(&new_leads, repository)
            .await?;

        tracing::info!("Detected conflicts for {} leads", batch_conflicts.len());

        let mut imported_leads = Vec::new();
        let mut disputed_leads = Vec::new();
        let mut skipped_duplicates = Vec::new();
        let mut validation_errors = Vec::new();

        // Process each lead
        for (index, new_lead) in new_leads.iter().enumerate() {
            // Check if this lead has conflicts
            let has_conflicts = batch_conflicts.iter().any(|(idx, _)| *idx == index);

            match self.process_single_lead(
                new_lead.clone(),
                has_conflicts,
                repository,
                enable_validation,
            ).await {
                Ok(ProcessedLead::Imported(lead)) => imported_leads.push(lead),
                Ok(ProcessedLead::Disputed(lead)) => disputed_leads.push(lead),
                Ok(ProcessedLead::Skipped(reason)) => skipped_duplicates.push(reason),
                Err(e) => {
                    validation_errors.push(format!("Failed to process {}: {}", new_lead.email, e));
                    tracing::warn!("Failed to process lead {}: {}", new_lead.email, e);
                }
            }
        }

        // Create conflict records for disputed leads
        for (lead_index, conflicts) in batch_conflicts {
            if let Some(_disputed_lead) = disputed_leads.iter().find(|l| {
                l.email == new_leads[lead_index].email
            }) {
                for mut conflict in conflicts {
                    conflict.lead_id = _disputed_lead.id;
                    if let Err(e) = repository.create_conflict(conflict).await {
                        tracing::warn!("Failed to create conflict record: {}", e);
                    }
                }
            }
        }

        let import_summary = ImportSummary {
            total_processed: new_leads.len(),
            successfully_imported: imported_leads.len(),
            marked_disputed: disputed_leads.len(),
            skipped_duplicates: skipped_duplicates.len(),
            validation_failures: validation_errors.len(),
        };

        tracing::info!("Import completed: {:?}", import_summary);

        Ok(CsvImportResult {
            imported_leads,
            disputed_leads,
            skipped_duplicates,
            validation_errors,
            import_summary,
        })
    }

    /// Process a single lead with conflict detection and validation
    async fn process_single_lead(
        &self,
        new_lead: NewLead,
        has_conflicts: bool,
        repository: &LeadRepository,
        enable_validation: bool,
    ) -> Result<ProcessedLead> {
        // Check for existing email
        if let Some(existing_lead) = repository.get_lead_by_email(&new_lead.email).await? {
            return Ok(ProcessedLead::Skipped(format!(
                "Email {} already exists for lead: {}",
                new_lead.email, existing_lead.name
            )));
        }

        // Validate email if enabled
        if enable_validation {
            let validation_result = self.validation_service.validate_email(&new_lead.email).await?;
            if validation_result == crate::database::models::ValidationResult::Invalid {
                return Err(AppError::validation(format!(
                    "Invalid email address: {}",
                    new_lead.email
                )));
            }
        }

        // Create the lead
        let created_lead = repository.create_lead(new_lead).await?;

        // If there are conflicts, mark as disputed
        if has_conflicts {
            let disputed_lead = repository
                .update_leads_status(vec![created_lead.id], crate::database::models::LeadStatus::Disputed)
                .await?;

            // Get the updated lead
            let updated_lead = repository
                .get_lead_by_id(created_lead.id)
                .await?
                .ok_or_else(|| AppError::internal("Failed to retrieve updated lead"))?;

            // Create validation record if enabled
            if enable_validation {
                if let Err(e) = self.validation_service.create_lead_validation(
                    created_lead.id,
                    &created_lead.email,
                    repository,
                ).await {
                    tracing::warn!("Failed to create validation record for lead {}: {}", created_lead.id, e);
                }
            }

            Ok(ProcessedLead::Disputed(updated_lead))
        } else {
            // Create validation record if enabled
            if enable_validation {
                if let Err(e) = self.validation_service.create_lead_validation(
                    created_lead.id,
                    &created_lead.email,
                    repository,
                ).await {
                    tracing::warn!("Failed to create validation record for lead {}: {}", created_lead.id, e);
                }
            }

            Ok(ProcessedLead::Imported(created_lead))
        }
    }

    /// Parse CSV data into NewLead structs
    fn parse_csv_data(&self, data: &Bytes) -> Result<Vec<NewLead>> {
        let cursor = Cursor::new(data);
        let mut reader = Reader::from_reader(cursor);
        let mut leads = Vec::new();
        let email_regex = Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").unwrap();

        // Get headers and validate required fields
        let headers = reader.headers()?.clone();
        
        let name_index = find_header_index(&headers, &["name", "full_name", "fullname", "lead_name", "business name"])?;
        let email_index = find_header_index(&headers, &["email", "email_address", "e-mail"])?;
        
        // Optional fields
        let notes_index = find_optional_header_index(&headers, &["notes", "note", "comment", "comments"]);
        let source_index = find_optional_header_index(&headers, &["source", "lead_source", "origin"]);
        let phone_index = find_optional_header_index(&headers, &["phone", "phone_number", "telephone", "mobile", "phone number"]);
        let company_index = find_optional_header_index(&headers, &["company", "organization", "business", "employer"]);

        for (row_number, result) in reader.records().enumerate() {
            let record = result.map_err(|e| {
                AppError::validation(format!("Error reading row {}: {}", row_number + 2, e))
            })?;

            // Extract required fields
            let name = record.get(name_index)
                .ok_or_else(|| AppError::validation(format!("Missing name in row {}", row_number + 2)))?
                .trim()
                .to_string();

            let email = record.get(email_index)
                .ok_or_else(|| AppError::validation(format!("Missing email in row {}", row_number + 2)))?
                .trim()
                .to_lowercase();

            // Validate required fields
            if name.is_empty() {
                return Err(AppError::validation(format!("Empty name in row {}", row_number + 2)));
            }

            if email.is_empty() {
                return Err(AppError::validation(format!("Empty email in row {}", row_number + 2)));
            }

            if !email_regex.is_match(&email) {
                return Err(AppError::validation(format!("Invalid email format in row {}: {}", row_number + 2, email)));
            }

            // Extract optional fields
            let notes = notes_index
                .and_then(|i| record.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let source = source_index
                .and_then(|i| record.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| Some("csv_import".to_string()));

            let phone = phone_index
                .and_then(|i| record.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let company = company_index
                .and_then(|i| record.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            leads.push(NewLead {
                name,
                email,
                notes,
                source,
                phone,
                company,
            });
        }

        if leads.is_empty() {
            return Err(AppError::validation("No valid leads found in CSV file"));
        }

        Ok(leads)
    }

    /// Generate enhanced CSV with conflict and validation information
    pub fn generate_enhanced_csv(&self, leads: &[LeadWithDetails]) -> Result<String> {
        let mut writer = Writer::from_writer(Vec::new());

        // Write enhanced headers
        writer.write_record(&[
            "id",
            "name", 
            "email", 
            "status", 
            "created_at", 
            "updated_at",
            "notes",
            "source",
            "phone",
            "company",
            "has_conflicts",
            "conflict_types",
            "validation_result",
            "validation_details",
        ])?;

        // Write data
        for lead_with_details in leads {
            let lead = &lead_with_details.lead;
            
            let has_conflicts = if lead_with_details.conflicts.is_empty() { "No" } else { "Yes" };
            
            let conflict_types = lead_with_details.conflicts
                .iter()
                .map(|c| c.conflict_type.to_string())
                .collect::<Vec<_>>()
                .join(", ");

            let (validation_result, validation_details) = if let Some(latest_validation) = 
                lead_with_details.validations.first() {
                (latest_validation.result.to_string(), latest_validation.details.as_deref().unwrap_or(""))
            } else {
                ("Not Validated".to_string(), "")
            };

            writer.write_record(&[
                lead.id.to_string(),
                lead.name.clone(),
                lead.email.clone(),
                lead.status.to_string(),
                lead.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                lead.updated_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                lead.notes.as_deref().unwrap_or("").to_string(),
                lead.source.as_deref().unwrap_or("").to_string(),
                lead.phone.as_deref().unwrap_or("").to_string(),
                lead.company.as_deref().unwrap_or("").to_string(),
                has_conflicts.to_string(),
                conflict_types,
                validation_result,
                validation_details.to_string(),
            ])?;
        }

        let data = writer.into_inner().map_err(|e| {
            AppError::internal(format!("Failed to create CSV: {}", e))
        })?;

        String::from_utf8(data).map_err(|e| {
            AppError::internal(format!("Failed to convert CSV to string: {}", e))
        })
    }
}

#[derive(Debug)]
enum ProcessedLead {
    Imported(Lead),
    Disputed(Lead),
    Skipped(String),
}

// Legacy function for backward compatibility
pub fn parse_csv_from_multipart(data: &Bytes) -> Result<Vec<NewLead>> {
    let service = EnhancedCsvService::new(
        crate::services::conflict_service::ConflictDetectionService::new(
            crate::database::models::ConflictDetectionSettings::default()
        ),
        crate::services::validation_service::create_default_validation_service(),
    );
    
    service.parse_csv_data(data)
}

// Enhanced CSV generation function
pub fn generate_csv(leads: &[Lead]) -> Result<String> {
    let mut writer = Writer::from_writer(Vec::new());

    // Write headers
    writer.write_record(&[
        "id",
        "name", 
        "email", 
        "status", 
        "created_at", 
        "updated_at",
        "notes",
        "source",
        "phone",
        "company"
    ])?;

    // Write data
    for lead in leads {
        writer.write_record(&[
            lead.id.to_string(),
            lead.name.clone(),
            lead.email.clone(),
            lead.status.to_string(),
            lead.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            lead.updated_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            lead.notes.as_deref().unwrap_or("").to_string(),
            lead.source.as_deref().unwrap_or("").to_string(),
            lead.phone.as_deref().unwrap_or("").to_string(),
            lead.company.as_deref().unwrap_or("").to_string(),
        ])?;
    }

    let data = writer.into_inner().map_err(|e| {
        AppError::internal(format!("Failed to create CSV: {}", e))
    })?;

    String::from_utf8(data).map_err(|e| {
        AppError::internal(format!("Failed to convert CSV to string: {}", e))
    })
}

fn find_header_index(headers: &csv::StringRecord, candidates: &[&str]) -> Result<usize> {
    for (i, header) in headers.iter().enumerate() {
        let header_lower = header.trim().to_lowercase();
        if candidates.iter().any(|&candidate| header_lower == *candidate) {
            return Ok(i);
        }
    }
    
    Err(AppError::validation(format!(
        "Required header not found. Expected one of: {}. Found headers: {}",
        candidates.join(", "),
        headers.iter().collect::<Vec<_>>().join(", ")
    )))
}

fn find_optional_header_index(headers: &csv::StringRecord, candidates: &[&str]) -> Option<usize> {
    for (i, header) in headers.iter().enumerate() {
        let header_lower = header.trim().to_lowercase();
        if candidates.iter().any(|&candidate| header_lower == *candidate) {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::{ConflictDetectionSettings, LeadStatus};

    #[tokio::test]
    async fn test_enhanced_csv_parsing() {
        let csv_data = "name,email\nJohn Doe,john@example.com\nJane Smith,jane@example.com";
        let bytes = Bytes::from(csv_data);
        
        let conflict_service = crate::services::conflict_service::ConflictDetectionService::new(
            ConflictDetectionSettings::default()
        );
        let validation_service = crate::services::validation_service::create_default_validation_service();
        let csv_service = EnhancedCsvService::new(conflict_service, validation_service);
        
        let result = csv_service.parse_csv_data(&bytes).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "John Doe");
        assert_eq!(result[0].email, "john@example.com");
    }

    #[test]
    fn test_enhanced_csv_generation() {
        let leads_with_details = vec![
            LeadWithDetails {
                lead: Lead {
                    id: 1,
                    name: "John Doe".to_string(),
                    email: "john@example.com".to_string(),
                    status: LeadStatus::New,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    notes: None,
                    source: Some("test".to_string()),
                    phone: None,
                    company: None,
                },
                conflicts: vec![],
                validations: vec![],
            }
        ];

        let conflict_service = crate::services::conflict_service::ConflictDetectionService::new(
            ConflictDetectionSettings::default()
        );
        let validation_service = crate::services::validation_service::create_default_validation_service();
        let csv_service = EnhancedCsvService::new(conflict_service, validation_service);

        let result = csv_service.generate_enhanced_csv(&leads_with_details).unwrap();
        assert!(result.contains("John Doe"));
        assert!(result.contains("john@example.com"));
        assert!(result.contains("has_conflicts"));
        assert!(result.contains("validation_result"));
    }
}