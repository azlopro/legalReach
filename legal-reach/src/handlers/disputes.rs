// New file: src/handlers/disputes.rs
use axum::{
    extract::{Path, State, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use validator::Validate;
use crate::{
    database::models::*,
    errors::AppError,
    services::{
        conflict_service::ConflictDetectionService,
        validation_service::EmailValidationService,
    },
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct DisputeQueryParams {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub conflict_type: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResolveDisputeRequest {
    #[validate(length(min = 1, message = "Resolution is required"))]
    pub resolution: String,
    
    #[validate(length(max = 1000, message = "Notes cannot exceed 1000 characters"))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct BulkResolveRequest {
    pub lead_ids: Vec<i32>,
    
    #[validate(length(min = 1, message = "Resolution is required"))]
    pub resolution: String,
    
    #[validate(length(max = 1000, message = "Notes cannot exceed 1000 characters"))]
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConflictAnalysisResponse {
    pub conflicts_detected: usize,
    pub leads_marked_disputed: usize,
    pub conflict_types: Vec<ConflictTypeSummary>,
}

#[derive(Debug, Serialize)]
pub struct ConflictTypeSummary {
    pub conflict_type: String,
    pub count: usize,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationResponse {
    pub validations_performed: usize,
    pub valid_emails: usize,
    pub invalid_emails: usize,
    pub unknown_results: usize,
}

#[derive(Debug, Serialize)]
pub struct DisputeStatsResponse {
    pub dispute_stats: DisputeStats,
    pub validation_stats: ValidationStats,
    pub recent_conflicts: Vec<RecentConflict>,
}

#[derive(Debug, Serialize)]
pub struct RecentConflict {
    pub lead_id: i32,
    pub lead_name: String,
    pub lead_email: String,
    pub conflict_type: String,
    pub conflict_details: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// GET /api/disputes - Get all disputed leads
pub async fn get_disputed_leads(
    State(state): State<AppState>,
    Query(params): Query<DisputeQueryParams>,
) -> Result<Json<PaginatedResponse<LeadWithDetails>>, AppError> {
    let pagination = PaginationParams {
        page: params.page,
        per_page: params.per_page,
    };

    let disputed_leads = state
        .lead_repository
        .get_disputed_leads(pagination)
        .await?;

    Ok(Json(disputed_leads))
}

// GET /api/disputes/stats - Get dispute and validation statistics
pub async fn get_dispute_stats(
    State(state): State<AppState>,
) -> Result<Json<DisputeStatsResponse>, AppError> {
    let dispute_stats = state.lead_repository.get_dispute_stats().await?;
    let validation_stats = state.lead_repository.get_validation_stats().await?;

    // Get recent conflicts for the dashboard
    let recent_conflicts_query = "
        SELECT 
            l.id as lead_id,
            l.name as lead_name,
            l.email as lead_email,
            c.conflict_type,
            c.conflict_details,
            c.created_at
        FROM lead_conflicts c
        JOIN leads l ON c.lead_id = l.id
        WHERE c.resolved_at IS NULL
        ORDER BY c.created_at DESC
        LIMIT 10
    ";

    let client = state.pool.get().await?;
    let rows = client.query(recent_conflicts_query, &[]).await?;
    
    let recent_conflicts: Vec<RecentConflict> = rows.into_iter().map(|row| {
        RecentConflict {
            lead_id: row.get("lead_id"),
            lead_name: row.get("lead_name"),
            lead_email: row.get("lead_email"),
            conflict_type: row.get("conflict_type"),
            conflict_details: row.get("conflict_details"),
            created_at: row.get("created_at"),
        }
    }).collect();

    Ok(Json(DisputeStatsResponse {
        dispute_stats,
        validation_stats,
        recent_conflicts,
    }))
}

// POST /api/disputes/{id}/resolve - Resolve a specific dispute
pub async fn resolve_dispute(
    State(state): State<AppState>,
    Path(lead_id): Path<i32>,
    Json(request): Json<ResolveDisputeRequest>,
) -> Result<Json<Lead>, AppError> {
    // Validate the request
    request.validate().map_err(|e| {
        AppError::validation(format!("Validation failed: {}", e))
    })?;

    // Parse resolution
    let resolution = match request.resolution.to_lowercase().as_str() {
        "accept" => DisputeResolution::Accept,
        "discard" => DisputeResolution::Discard,
        "mark_contacted" => DisputeResolution::MarkContacted,
        _ => return Err(AppError::validation("Invalid resolution. Must be 'accept', 'discard', or 'mark_contacted'")),
    };

    // Check if lead exists and is disputed
    let lead = state
        .lead_repository
        .get_lead_by_id(lead_id)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;

    if lead.status != LeadStatus::Disputed {
        return Err(AppError::validation("Lead is not in disputed status"));
    }

    // Resolve the dispute
    let updated_lead = state
        .lead_repository
        .resolve_dispute(lead_id, resolution, request.notes)
        .await?;

    Ok(Json(updated_lead))
}

// POST /api/disputes/bulk-resolve - Resolve multiple disputes
pub async fn bulk_resolve_disputes(
    State(state): State<AppState>,
    Json(request): Json<BulkResolveRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate the request
    request.validate().map_err(|e| {
        AppError::validation(format!("Validation failed: {}", e))
    })?;

    if request.lead_ids.is_empty() {
        return Err(AppError::validation("No lead IDs provided"));
    }

    if request.lead_ids.len() > 100 {
        return Err(AppError::validation("Cannot resolve more than 100 disputes at once"));
    }

    // Parse resolution
    let resolution = match request.resolution.to_lowercase().as_str() {
        "accept" => DisputeResolution::Accept,
        "discard" => DisputeResolution::Discard,
        "mark_contacted" => DisputeResolution::MarkContacted,
        _ => return Err(AppError::validation("Invalid resolution. Must be 'accept', 'discard', or 'mark_contacted'")),
    };

    let mut resolved_count = 0;
    let mut errors = Vec::new();

    for lead_id in request.lead_ids {
        match state
            .lead_repository
            .resolve_dispute(lead_id, resolution.clone(), request.notes.clone())
            .await
        {
            Ok(_) => resolved_count += 1,
            Err(e) => {
                errors.push(format!("Failed to resolve dispute for lead {}: {}", lead_id, e));
                tracing::warn!("Failed to resolve dispute for lead {}: {}", lead_id, e);
            }
        }
    }

    Ok(Json(serde_json::json!({
        "resolved_count": resolved_count,
        "errors": errors,
        "status": if errors.is_empty() { "success" } else { "partial_success" }
    })))
}

// POST /api/disputes/analyze - Analyze existing leads for conflicts
pub async fn analyze_conflicts(
    State(state): State<AppState>,
) -> Result<Json<ConflictAnalysisResponse>, AppError> {
    // Get conflict detection settings
    let settings = ConflictDetectionSettings::default(); // In production, load from database
    let conflict_service = ConflictDetectionService::new(settings);

    // Analyze existing leads
    let conflicts = conflict_service
        .analyze_existing_leads(&state.lead_repository)
        .await?;

    let mut leads_marked_disputed = 0;
    let mut conflict_type_counts = std::collections::HashMap::new();

    // Create conflicts and mark leads as disputed
    for mut conflict in conflicts {
        // Check if the lead is already disputed
        if let Ok(Some(lead)) = state.lead_repository.get_lead_by_id(conflict.lead_id).await {
            if lead.status == LeadStatus::Disputed {
                continue; // Skip if already disputed
            }
        }

        // Create the conflict record
        if let Ok(_) = state.lead_repository.create_conflict(conflict.clone()).await {
            // Update lead status to disputed
            if let Ok(_) = state
                .lead_repository
                .update_leads_status(vec![conflict.lead_id], LeadStatus::Disputed)
                .await
            {
                leads_marked_disputed += 1;
            }

            // Count conflict types
            *conflict_type_counts.entry(conflict.conflict_type.to_string()).or_insert(0) += 1;
        }
    }

    let conflict_types: Vec<ConflictTypeSummary> = conflict_type_counts
        .into_iter()
        .map(|(conflict_type, count)| {
            let description = match conflict_type.as_str() {
                "same_domain" => "Multiple leads from the same email domain",
                "duplicate_email" => "Exact email address duplicates",
                "similar_name" => "Similar or potentially duplicate names",
                _ => "Unknown conflict type",
            };

            ConflictTypeSummary {
                conflict_type,
                count,
                description: description.to_string(),
            }
        })
        .collect();

    Ok(Json(ConflictAnalysisResponse {
        conflicts_detected: conflict_types.iter().map(|ct| ct.count).sum(),
        leads_marked_disputed,
        conflict_types,
    }))
}

// POST /api/disputes/validate-emails - Validate emails for all leads
pub async fn validate_all_emails(
    State(state): State<AppState>,
) -> Result<Json<ValidationResponse>, AppError> {
    // Create validation service
    let validation_service = crate::services::validation_service::create_default_validation_service();

    // Validate all leads
    let validations_performed = validation_service
        .validate_all_leads(&state.lead_repository)
        .await?;

    // Get updated validation stats
    let validation_stats = state.lead_repository.get_validation_stats().await?;

    Ok(Json(ValidationResponse {
        validations_performed,
        valid_emails: validation_stats.valid_emails as usize,
        invalid_emails: validation_stats.invalid_emails as usize,
        unknown_results: (validation_stats.total_validations - validation_stats.valid_emails - validation_stats.invalid_emails) as usize,
    }))
}

// GET /api/disputes/leads/{id}/details - Get detailed information about a disputed lead
pub async fn get_lead_dispute_details(
    State(state): State<AppState>,
    Path(lead_id): Path<i32>,
) -> Result<Json<LeadWithDetails>, AppError> {
    // Get the lead
    let lead = state
        .lead_repository
        .get_lead_by_id(lead_id)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;

    // Get conflicts for the lead
    let conflicts = state
        .lead_repository
        .get_conflicts_for_lead(lead_id)
        .await?;

    // Get validations for the lead
    let validations = state
        .lead_repository
        .get_validations_for_lead(lead_id)
        .await?;

    Ok(Json(LeadWithDetails {
        lead,
        conflicts,
        validations,
    }))
}

// POST /api/disputes/leads/{id}/validate - Validate a specific lead's email
pub async fn validate_lead_email(
    State(state): State<AppState>,
    Path(lead_id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Get the lead
    let lead = state
        .lead_repository
        .get_lead_by_id(lead_id)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;

    // Create validation service
    let validation_service = crate::services::validation_service::create_default_validation_service();

    // Validate the email
    let result = validation_service.validate_email(&lead.email).await?;
    let details = validation_service.get_validation_details(&lead.email, &result).await;

    // Create validation record
    let validation = NewLeadValidation {
        lead_id,
        validation_type: ValidationType::EmailActive,
        result: result.clone(),
        details,
    };

    let created_validation = state
        .lead_repository
        .create_validation(validation)
        .await?;

    Ok(Json(serde_json::json!({
        "validation": created_validation,
        "lead": lead
    })))
}

// DELETE /api/disputes/{id} - Delete a disputed lead (admin only)
pub async fn delete_disputed_lead(
    State(state): State<AppState>,
    Path(lead_id): Path<i32>,
) -> Result<impl IntoResponse, AppError> {
    // Check if lead exists and is disputed
    let lead = state
        .lead_repository
        .get_lead_by_id(lead_id)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;

    if lead.status != LeadStatus::Disputed {
        return Err(AppError::validation("Lead is not in disputed status"));
    }

    // Delete the lead (this will cascade delete conflicts and validations)
    let deleted = state.lead_repository.delete_lead(lead_id).await?;

    if deleted {
        Ok((StatusCode::NO_CONTENT, ()).into_response())
    } else {
        Err(AppError::not_found("Lead"))
    }
}