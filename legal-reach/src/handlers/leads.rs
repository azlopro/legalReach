// Simplified handlers/leads.rs that should work
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
    services::csv_service::{parse_csv_from_multipart, generate_csv},
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
pub struct BulkImportResponse {
    pub imported_count: usize,
    pub skipped_count: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LeadStatsResponse {
    pub total_leads: i64,
    pub new_leads: i64,
    pub contacted_leads: i64,
    pub qualified_leads: i64,
    pub converted_leads: i64,
    pub lost_leads: i64,
}

#[derive(Debug, Deserialize)]
pub struct LeadsQueryParams {
    pub status: Option<String>,
    pub search: Option<String>,
    pub source: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

// GET /api/leads - Get leads with filtering and pagination
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
            _ => None,
        }
    });

    let filters = LeadFilters {
        status,
        search: params.search,
        source: params.source,
        created_after: None,
        created_before: None,
    };

    let pagination = PaginationParams {
        page: params.page.or(Some(1)),
        per_page: params.per_page.or(Some(50)),
    };

    let result = state.lead_repository.get_leads(filters, pagination).await?;
    Ok(Json(result))
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

// POST /api/leads - Create a new lead
pub async fn create_lead(
    State(state): State<AppState>,
    Json(request): Json<CreateLeadRequest>,
) -> Result<(StatusCode, Json<Lead>), AppError> {
    // Validate the request
    request.validate().map_err(|e| {
        AppError::validation(format!("Validation failed: {}", e))
    })?;

    let new_lead = NewLead {
        name: request.name,
        email: request.email,
        notes: request.notes,
        source: request.source.or_else(|| Some("api".to_string())),
        phone: request.phone,
        company: request.company,
    };

    let lead = state.lead_repository.create_lead(new_lead).await?;
    
    Ok((StatusCode::CREATED, Json(lead)))
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
        email: request.email,
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

// POST /api/leads/import - Import leads from CSV
pub async fn import_leads_csv(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<BulkImportResponse>, AppError> {
    let mut imported_count = 0;
    let mut skipped_count = 0;
    let mut errors = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => {
                return Err(AppError::validation(format!("Failed to read multipart data: {}", e)));
            }
        };

        let name = field.name().unwrap_or("").to_string();
        
        if name == "file" {
            let filename = field.file_name().unwrap_or("").to_string();
            
            if !filename.to_lowercase().ends_with(".csv") {
                return Err(AppError::validation("Only CSV files are allowed"));
            }

            let data = field.bytes().await.map_err(|e| {
                AppError::validation(format!("Failed to read file data: {}", e))
            })?;

            let new_leads = parse_csv_from_multipart(&data).map_err(|e| {
                AppError::validation(format!("Failed to parse CSV: {}", e))
            })?;

            if new_leads.len() > state.config.features.max_leads_per_import {
                return Err(AppError::validation(format!(
                    "Cannot import more than {} leads at once",
                    state.config.features.max_leads_per_import
                )));
            }

            for new_lead in new_leads {
                match state.lead_repository.create_lead(new_lead.clone()).await {
                    Ok(_) => imported_count += 1,
                    Err(AppError::Conflict { .. }) => {
                        skipped_count += 1;
                        errors.push(format!("Duplicate email: {}", new_lead.email));
                    }
                    Err(e) => {
                        errors.push(format!("Failed to import {}: {}", new_lead.email, e));
                    }
                }
            }
            break;
        }
    }

    Ok(Json(BulkImportResponse {
        imported_count,
        skipped_count,
        errors,
    }))
}

// GET /api/leads/export - Export leads to CSV
pub async fn export_leads_csv(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let all_leads = state.lead_repository.get_leads(
        LeadFilters {
            status: None,
            search: None,
            source: None,
            created_after: None,
            created_before: None,
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

    let csv_data = generate_csv(&leads)?;
    let filename = format!("leads_export_{}.csv", chrono::Utc::now().format("%Y%m%d_%H%M%S"));

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "text/csv".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", filename).parse().unwrap(),
    );

    Ok((headers, csv_data))
}

// GET /api/leads/stats - Get lead statistics
#[axum::debug_handler]
pub async fn get_lead_stats(
    State(state): State<AppState>,
) -> Result<Json<LeadStatsResponse>, AppError> {
    let all_leads = state.lead_repository.get_leads(
        LeadFilters {
            status: None,
            search: None,
            source: None,
            created_after: None,
            created_before: None,
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

    for lead in all_leads.data {
        match lead.status {
            LeadStatus::New => new_leads += 1,
            LeadStatus::Contacted => contacted_leads += 1,
            LeadStatus::Qualified => qualified_leads += 1,
            LeadStatus::Converted => converted_leads += 1,
            LeadStatus::Lost => lost_leads += 1,
        }
    }

    Ok(Json(LeadStatsResponse {
        total_leads,
        new_leads,
        contacted_leads,
        qualified_leads,
        converted_leads,
        lost_leads,
    }))
}