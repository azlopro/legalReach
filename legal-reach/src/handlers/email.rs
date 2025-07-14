// Fixed handlers/email.rs
use axum::{
    extract::{State, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use validator::Validate;
use crate::{
    database::models::*,
    errors::AppError,
    services::email_service::send_email_to_leads,
    AppState,
};

#[derive(Debug, Deserialize, Validate)]
pub struct SendEmailRequest {
    pub lead_ids: Vec<i32>,
    
    #[validate(length(min = 1, max = 500, message = "Subject must be between 1 and 500 characters"))]
    pub subject: String,
    
    #[validate(length(min = 1, max = 10000, message = "Body must be between 1 and 10000 characters"))]
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct SendEmailResponse {
    pub emails_sent: usize,
    pub emails_failed: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct EmailLogResponse {
    pub logs: Vec<EmailLog>,
}

// POST /api/email/send - Send emails to selected leads
pub async fn send_emails(
    State(state): State<AppState>,
    Json(request): Json<SendEmailRequest>,
) -> impl IntoResponse {
    // Validate the request
    if let Err(e) = request.validate() {
        return AppError::validation(format!("Validation failed: {}", e)).into_response();
    }

    if request.lead_ids.is_empty() {
        return AppError::validation("No lead IDs provided").into_response();
    }

    if request.lead_ids.len() > 100 {
        return AppError::validation("Cannot send emails to more than 100 leads at once").into_response();
    }

    // Check if email sending is enabled
    if !state.config.features.enable_email_sending {
        return AppError::forbidden("Email sending is currently disabled").into_response();
    }

    // Get the leads
    let leads = match state.lead_repository.get_leads_by_ids(request.lead_ids).await {
        Ok(leads) => leads,
        Err(e) => return e.into_response(),
    };
    
    if leads.is_empty() {
        return AppError::not_found("No leads found with provided IDs").into_response();
    }

    // Send emails and track results
    let (emails_sent, _emails_failed, errors) = match send_email_to_leads(
        &state.config,
        &leads,
        &request.subject,
        &request.body,
    ).await {
        Ok(result) => result,
        Err(e) => return e.into_response(),
    };

    // Update lead statuses to 'contacted' for successfully sent emails
    let successful_lead_ids: Vec<i32> = leads
        .iter()
        .take(emails_sent)
        .map(|lead| lead.id)
        .collect();

    if !successful_lead_ids.is_empty() {
        // Only update leads that are currently 'new' to avoid overwriting other statuses
        let new_leads: Vec<i32> = leads
            .iter()
            .filter(|lead| lead.status == LeadStatus::New)
            .take(emails_sent)
            .map(|lead| lead.id)
            .collect();

        if !new_leads.is_empty() {
            if let Err(e) = state
                .lead_repository
                .update_leads_status(new_leads, LeadStatus::Contacted)
                .await 
            {
                tracing::warn!("Failed to update lead statuses: {}", e);
            }
        }

        // Create email logs for successful sends
        for lead in leads.iter().take(emails_sent) {
            let email_log = NewEmailLog {
                lead_id: lead.id,
                subject: request.subject.clone(),
                body: request.body.clone(),
            };

            if let Err(e) = state.lead_repository.create_email_log(email_log).await {
                tracing::warn!("Failed to create email log for lead {}: {}", lead.id, e);
            }
        }
    }

    (StatusCode::OK, Json(SendEmailResponse {
        emails_sent,
        emails_failed: errors.len(),
        errors,
    })).into_response()
}

// GET /api/email/logs/:lead_id - Get email logs for a specific lead
pub async fn get_email_logs(
    State(state): State<AppState>,
    Path(lead_id): Path<i32>,
) -> impl IntoResponse {
    // Verify the lead exists
    let _lead = match state.lead_repository.get_lead_by_id(lead_id).await {
        Ok(Some(lead)) => lead,
        Ok(None) => return AppError::not_found("Lead").into_response(),
        Err(e) => return e.into_response(),
    };

    let logs = match state.lead_repository.get_email_logs_for_lead(lead_id).await {
        Ok(logs) => logs,
        Err(e) => return e.into_response(),
    };

    (StatusCode::OK, Json(EmailLogResponse { logs })).into_response()
}

// POST /api/email/test - Test email configuration
pub async fn test_email_config(
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !state.config.features.enable_email_sending {
        return AppError::forbidden("Email sending is currently disabled").into_response();
    }

    // Create a test lead for email testing
    let test_lead = Lead {
        id: 0,
        name: "Test User".to_string(),
        email: state.config.email.from_email.clone(),
        status: LeadStatus::New,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        notes: None,
        source: Some("test".to_string()),
        phone: None,
        company: None,
    };

    let test_subject = "Email Configuration Test";
    let test_body = "This is a test email to verify your email configuration is working correctly.";

    let (emails_sent, _emails_failed, errors) = match send_email_to_leads(
        &state.config,
        &vec![test_lead],
        test_subject,
        test_body,
    ).await {
        Ok(result) => result,
        Err(e) => return e.into_response(),
    };

    if emails_sent > 0 {
        (StatusCode::OK, Json(serde_json::json!({
            "status": "success",
            "message": "Test email sent successfully"
        }))).into_response()
    } else {
        (StatusCode::OK, Json(serde_json::json!({
            "status": "error",
            "message": "Failed to send test email",
            "errors": errors
        }))).into_response()
    }
}