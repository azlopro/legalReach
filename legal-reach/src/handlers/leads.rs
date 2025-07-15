// Enhanced src/handlers/leads.rs with conflict detection and validation
use axum::{
    extract::{Path, State, Multipart, Query},
    http::{StatusCode, HeaderMap, header},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use validator::Validate;
use crate::{
    database::models::*,
    errors::AppError,
    services::{
        csv_service::EnhancedCsvService,
        conflict_service::ConflictDetectionService,
        validation_service::EmailValidationService,
    },
    AppState,
};

#[derive(Debug, Deserialize, Validate)]
pub struct CreateLeadRequest {
    #[validate(length(min = 1, max = 255, message = "Name must be between 1 and 255 characters"))]
    pub name: String,
    
    #[validate(email(message = "Invalid email format"))]
    pub email: String,
    
    #[validate(length(max = 1000, message = "Notes cannot exceed 1000 characters"))]
    pub notes: Option<String>,
    
    #[validate(length(max = 100, message = "Source cannot exceed 100 characters"))]
    pub source: Option<String>,
    
    #[validate(length(max = 50, message = "Phone cannot exceed 50 characters"))]
    pub phone: Option<String>,
    
    #[validate(length(max = 255, message = "Company cannot exceed 255 characters"))]
    pub company: Option<String>,
    
    pub validate_email: Option<bool>,
    pub check_conflicts: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateLeadRequest {
    #[validate(length(min = 1, max = 255, message = "Name must be between 1 and 255 characters"))]
    pub name: Option<String>,
    
    #[validate(email(message = "Invalid email format"))]
    pub email: Option<String>,
    
    pub status: Option<LeadStatus>,
    
    #[validate(length(max = 1000, message = "Notes cannot exceed 1000 characters"))]
    pub notes: Option<String>,
    
    #[validate(length(max = 50, message = "Phone cannot exceed 50 characters"))]
    pub phone: Option<String>,
    
    #[validate(length(max = 255, message = "Company cannot exceed 255 characters"))]
    pub company: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BulkUpdateRequest {
    pub lead_ids: Vec<i32>,
    pub status: LeadStatus,
}

#[derive(Debug, Serialize)]
pub struct EnhancedBulkImportResponse {
    pub total_processed: usize,
    pub successfully_imported: usize,
    pub marked_disputed: usize,
    pub skipped_duplicates: usize,
    pub validation_failures: usize,
    pub imported_leads: Vec<Lead>,
    pub disputed_leads: Vec<Lead>,
    pub errors: Vec<String>,
    pub conflict_summary: Vec<ConflictSummary>,
}

#[derive(Debug, Serialize)]
pub struct ConflictSummary {
    pub conflict_type: String,
    pub count: usize,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct EnhancedLeadStatsResponse {
    pub total_leads: i64,
    pub new_leads: i64,
    pub contacted_leads: i64,
    pub qualified_leads: i64,
    pub converted_leads: i64,
    pub lost_leads: i64,
    pub disputed_leads: i64,
    pub dispute_stats: DisputeStats,
    pub validation_stats: ValidationStats,
}

#[derive(Debug, Deserialize)]
pub struct LeadsQueryParams {
    pub status: Option<String>,
    pub search: Option<String>,
    pub source: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub has_conflicts: Option<bool>,
    pub validation_result: Option<String>,
    pub include_details: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ImportOptions {
    pub enable_validation: Option<bool>,
    pub enable_conflict_detection: Option<bool>,
    pub auto_mark_disputed: Option<bool>,
}

// GET /api/leads - Get leads with enhanced filtering
pub async fn get_leads(
    State(state): State<AppState>,
    Query(params): Query<LeadsQueryParams>,
) -> Result<Json<PaginatedResponse<Lead>>, AppError> {
    let status = params.status.and_then(|s| {
        match s.to_lowercase().as_str() {
            "new" => Some(LeadStatus::New),
            "contacted" => Some(LeadStatus::Contacted),
            "qualified" => Some(LeadStatus::Qualified),
            "converted" => Some(LeadStatus::Converted),
            "lost" => Some(LeadStatus::Lost),
            "disputed" => Some(LeadStatus::Disputed),
            _ => None,
        }
    });

    let validation_result = params.validation_result.and_then(|v| {
        match v.to_lowercase().as_str() {
            "valid" => Some(ValidationResult::Valid),
            "invalid" => Some(ValidationResult::Invalid),
            "unknown" => Some(ValidationResult::Unknown),
            "pending" => Some(ValidationResult::Pending),
            _ => None,
        }
    });

    let filters = LeadFilters {
        status,
        search: params.search,
        source: params.source,
        created_after: None,
        created_before: None,
        has_conflicts: params.has_conflicts,
        validation_result,
    };

    let pagination = PaginationParams {
        page: params.page.or(Some(1)),
        per_page: params.per_page.or(Some(50)),
    };

    let result = state.lead_repository.get_leads(filters, pagination).await?;
    Ok(Json(result))
}

// GET /api/leads/with-details - Get leads with conflict and validation details
pub async fn get_leads_with_details(
    State(state): State<AppState>,
    Query(params): Query<LeadsQueryParams>,
) -> Result<Json<PaginatedResponse<LeadWithDetails>>, AppError> {
    // First get the regular leads
    let leads_response = get_leads(State(state.clone()), Query(params)).await?;
    let leads = leads_response.0.data;

    let mut leads_with_details = Vec::new();

    // Get details for each lead
    for lead in leads {
        let conflicts = state.lead_repository.get_conflicts_for_lead(lead.id).await?;
        let validations = state.lead_repository.get_validations_for_lead(lead.id).await?;

        leads_with_details.push(LeadWithDetails {
            lead,
            conflicts,
            validations,
        });
    }

    let response = PaginatedResponse {
        data: leads_with_details,
        pagination: leads_response.0.pagination,
    };

    Ok(Json(response))
}

// GET /api/leads/:id - Get a specific lead
pub async fn get_lead(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Lead>, AppError> {
    let lead = state
        .lead_repository
        .get_lead_by_id(id)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;
    
    Ok(Json(lead))
}

// GET /api/leads/:id/details - Get a specific lead with all details
pub async fn get_lead_with_details(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<LeadWithDetails>, AppError> {
    let lead = state
        .lead_repository
        .get_lead_by_id(id)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;

    let conflicts = state.lead_repository.get_conflicts_for_lead(id).await?;
    let validations = state.lead_repository.get_validations_for_lead(id).await?;

    Ok(Json(LeadWithDetails {
        lead,
        conflicts,
        validations,
    }))
}

// POST /api/leads - Create a new lead with conflict detection
pub async fn create_lead(
    State(state): State<AppState>,
    Json(request): Json<CreateLeadRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    // Validate the request
    request.validate().map_err(|e| {
        AppError::validation(format!("Validation failed: {}", e))
    })?;

    let new_lead = NewLead {
        name: request.name,
        email: request.email.to_lowercase(),
        notes: request.notes,
        source: request.source.or_else(|| Some("api".to_string())),
        phone: request.phone,
        company: request.company,
    };

    // Check for conflicts if enabled
    let check_conflicts = request.check_conflicts.unwrap_or(true);
    let validate_email = request.validate_email.unwrap_or(false);

    let mut conflicts = Vec::new();
    let mut validation_result = None;

    if check_conflicts {
        let conflict_service = ConflictDetectionService::new(
            ConflictDetectionSettings::default() // In production, load from database
        );
        conflicts = conflict_service
            .detect_conflicts_for_lead(&new_lead, &state.lead_repository)
            .await?;
    }

    if validate_email {
        let validation_service = EmailValidationService::new(
            crate::services::validation_service::ValidationSettings::default()
        );
        validation_result = Some(validation_service.validate_email(&new_lead.email).await?);
        
        // Reject if email is invalid
        if let Some(ValidationResult::Invalid) = validation_result {
            return Err(AppError::validation("Email address is invalid"));
        }
    }

    // Create the lead
    let mut created_lead = state.lead_repository.create_lead(new_lead).await?;

    // If there are conflicts, mark as disputed
    if !conflicts.is_empty() {
        state
            .lead_repository
            .update_leads_status(vec![created_lead.id], LeadStatus::Disputed)
            .await?;
        
        created_lead.status = LeadStatus::Disputed;

        // Create conflict records
        for conflict in &conflicts {
            let mut new_conflict = conflict.clone();
            new_conflict.lead_id = created_lead.id;
            if let Err(e) = state.lead_repository.create_conflict(new_conflict).await {
                tracing::warn!("Failed to create conflict record: {}", e);
            }
        }
    }

    // Create validation record if email was validated
    if let Some(ref result) = validation_result {
        let validation = NewLeadValidation {
            lead_id: created_lead.id,
            validation_type: ValidationType::EmailActive,
            result: result.clone(),
            details: None,
        };

        if let Err(e) = state.lead_repository.create_validation(validation).await {
            tracing::warn!("Failed to create validation record: {}", e);
        }
    }

    let response = serde_json::json!({
        "lead": created_lead,
        "has_conflicts": !conflicts.is_empty(),
        "conflict_count": conflicts.len(),
        "validation_performed": validate_email,
        "validation_result": validation_result,
        "status": if conflicts.is_empty() { "created" } else { "created_with_disputes" }
    });

    Ok((StatusCode::CREATED, Json(response)))
}

// POST /api/leads/:id/update - Update a lead
pub async fn update_lead(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(request): Json<UpdateLeadRequest>,
) -> Result<Json<Lead>, AppError> {
    // Validate the request
    request.validate().map_err(|e| {
        AppError::validation(format!("Validation failed: {}", e))
    })?;

    let update = UpdateLead {
        name: request.name,
        email: request.email.map(|e| e.to_lowercase()),
        status: request.status,
        notes: request.notes,
        phone: request.phone,
        company: request.company,
    };

    let lead = state
        .lead_repository
        .update_lead(id, update)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;

    Ok(Json(lead))
}

// POST /api/leads/bulk-update - Update multiple leads' status
pub async fn bulk_update_leads(
    State(state): State<AppState>,
    Json(request): Json<BulkUpdateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if request.lead_ids.is_empty() {
        return Err(AppError::validation("No lead IDs provided"));
    }

    if request.lead_ids.len() > 100 {
        return Err(AppError::validation("Cannot update more than 100 leads at once"));
    }

    let updated_count = state
        .lead_repository
        .update_leads_status(request.lead_ids, request.status)
        .await?;

    Ok(Json(serde_json::json!({
        "updated_count": updated_count,
        "status": "success"
    })))
}

// POST /api/leads/import - Enhanced CSV import with conflict detection
pub async fn import_leads_csv(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<EnhancedBulkImportResponse>, AppError> {
    let mut import_options = ImportOptions {
        enable_validation: Some(false),
        enable_conflict_detection: Some(true),
        auto_mark_disputed: Some(true),
    };

    let mut csv_data: Option<bytes::Bytes> = None;

    // Parse multipart form
    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        
        match name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("").to_string();
                
                if !filename.to_lowercase().ends_with(".csv") {
                    return Err(AppError::validation("Only CSV files are allowed"));
                }

                csv_data = Some(field.bytes().await.map_err(|e| {
                    AppError::validation(format!("Failed to read file data: {}", e))
                })?);
            }
            "enable_validation" => {
                let value = field.text().await.unwrap_or_default();
                import_options.enable_validation = Some(value.to_lowercase() == "true");
            }
            "enable_conflict_detection" => {
                let value = field.text().await.unwrap_or_default();
                import_options.enable_conflict_detection = Some(value.to_lowercase() == "true");
            }
            "auto_mark_disputed" => {
                let value = field.text().await.unwrap_or_default();
                import_options.auto_mark_disputed = Some(value.to_lowercase() == "true");
            }
            _ => {
                // Skip unknown fields
                let _ = field.text().await;
            }
        }
    }

    let data = csv_data.ok_or_else(|| {
        AppError::validation("No CSV file provided")
    })?;

    // Check file size limit
    if data.len() > state.config.server.max_request_size {
        return Err(AppError::validation("File too large"));
    }

    // Create services
    let conflict_service = ConflictDetectionService::new(
        ConflictDetectionSettings::default() // In production, load from database
    );
    let validation_service = EmailValidationService::new(
        crate::services::validation_service::ValidationSettings::default()
    );
    let csv_service = EnhancedCsvService::new(conflict_service, validation_service);

    // Import leads with conflict detection and validation
    let import_result = csv_service
        .import_leads_with_conflict_detection(
            &data,
            &state.lead_repository,
            import_options.enable_validation.unwrap_or(false),
        )
        .await?;

    // Create conflict summary
    let mut conflict_types = std::collections::HashMap::new();
    for disputed_lead in &import_result.disputed_leads {
        let conflicts = state.lead_repository.get_conflicts_for_lead(disputed_lead.id).await?;
        for conflict in conflicts {
            *conflict_types.entry(conflict.conflict_type.to_string()).or_insert(0) += 1;
        }
    }

    let conflict_summary: Vec<ConflictSummary> = conflict_types
        .into_iter()
        .map(|(conflict_type, count)| {
            let description = match conflict_type.as_str() {
                "same_domain" => "Multiple leads from the same email domain",
                "duplicate_email" => "Exact email address duplicates",
                "similar_name" => "Similar or potentially duplicate names",
                _ => "Unknown conflict type",
            };

            ConflictSummary {
                conflict_type,
                count,
                description: description.to_string(),
            }
        })
        .collect();

    Ok(Json(EnhancedBulkImportResponse {
        total_processed: import_result.import_summary.total_processed,
        successfully_imported: import_result.import_summary.successfully_imported,
        marked_disputed: import_result.import_summary.marked_disputed,
        skipped_duplicates: import_result.import_summary.skipped_duplicates,
        validation_failures: import_result.import_summary.validation_failures,
        imported_leads: import_result.imported_leads,
        disputed_leads: import_result.disputed_leads,
        errors: import_result.validation_errors,
        conflict_summary,
    }))
}

// GET /api/leads/export - Enhanced CSV export
pub async fn export_leads_csv(
    State(state): State<AppState>,
    Query(params): Query<LeadsQueryParams>,
) -> Result<impl IntoResponse, AppError> {
    let include_details = params.include_details.unwrap_or(false);

    if include_details {
        // Export with details
        let leads_with_details = get_leads_with_details(State(state), Query(params)).await?.0;
        
        if leads_with_details.data.is_empty() {
            return Err(AppError::not_found("No leads found"));
        }

        let conflict_service = ConflictDetectionService::new(
            ConflictDetectionSettings::default()
        );
        let validation_service = EmailValidationService::new(
            crate::services::validation_service::ValidationSettings::default()
        );
        let csv_service = EnhancedCsvService::new(conflict_service, validation_service);

        let csv_data = csv_service.generate_enhanced_csv(&leads_with_details.data)?;
        let filename = format!("leads_detailed_export_{}.csv", chrono::Utc::now().format("%Y%m%d_%H%M%S"));

        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "text/csv".parse().unwrap());
        headers.insert(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename).parse().unwrap(),
        );

        Ok((headers, csv_data))
    } else {
        // Regular export
        let all_leads = state.lead_repository.get_leads(
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

        let leads = all_leads.data;
        
        if leads.is_empty() {
            return Err(AppError::not_found("No leads found"));
        }

        let csv_data = crate::services::csv_service::generate_csv(&leads)?;
        let filename = format!("leads_export_{}.csv", chrono::Utc::now().format("%Y%m%d_%H%M%S"));

        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "text/csv".parse().unwrap());
        headers.insert(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename).parse().unwrap(),
        );

        Ok((headers, csv_data))
    }
}

// GET /api/leads/stats - Enhanced lead statistics
pub async fn get_lead_stats(
    State(state): State<AppState>,
) -> Result<Json<EnhancedLeadStatsResponse>, AppError> {
    let all_leads = state.lead_repository.get_leads(
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

    let total_leads = all_leads.pagination.total_items;
    
    let mut new_leads = 0;
    let mut contacted_leads = 0;
    let mut qualified_leads = 0;
    let mut converted_leads = 0;
    let mut lost_leads = 0;
    let mut disputed_leads = 0;

    for lead in all_leads.data {
        match lead.status {
            LeadStatus::New => new_leads += 1,
            LeadStatus::Contacted => contacted_leads += 1,
            LeadStatus::Qualified => qualified_leads += 1,
            LeadStatus::Converted => converted_leads += 1,
            LeadStatus::Lost => lost_leads += 1,
            LeadStatus::Disputed => disputed_leads += 1,
        }
    }

    let dispute_stats = state.lead_repository.get_dispute_stats().await?;
    let validation_stats = state.lead_repository.get_validation_stats().await?;

    Ok(Json(EnhancedLeadStatsResponse {
        total_leads,
        new_leads,
        contacted_leads,
        qualified_leads,
        converted_leads,
        lost_leads,
        disputed_leads,
        dispute_stats,
        validation_stats,
    }))
}