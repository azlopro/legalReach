use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;
use crate::{
    database::models::*,
    errors::{AppError, Result},
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
) -> Result<Json<SendEmailResponse>> {
    // Validate the request
    request.validate().map_err(|e| {
        AppError::validation(format!("Validation failed: {}", e))
    })?;

    if request.lead_ids.is_empty() {
        return Err(AppError::validation("No lead IDs provided"));
    }

    if request.lead_ids.len() > 100 {
        return Err(AppError::validation("Cannot send emails to more than 100 leads at once"));
    }

    // Check if email sending is enabled
    if !state.config.features.enable_email_sending {
        return Err(AppError::forbidden("Email sending is currently disabled"));
    }

    // Get the leads
    let leads = state.lead_repository.get_leads_by_ids(request.lead_ids).await?;
    
    if leads.is_empty() {
        return Err(AppError::not_found("No leads found with provided IDs"));
    }

    // Send emails and track results
    let (emails_sent, _emails_failed, errors) = send_email_to_leads(
        &state.config,
        &leads,
        &request.subject,
        &request.body,
    ).await?;

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
            state
                .lead_repository
                .update_leads_status(new_leads, LeadStatus::Contacted)
                .await?;
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

    Ok(Json(SendEmailResponse {
        emails_sent,
        emails_failed: errors.len(),
        errors,
    }))
}

// GET /api/email/logs/:lead_id - Get email logs for a specific lead
pub async fn get_email_logs(
    State(state): State<AppState>,
    axum::extract::Path(lead_id): axum::extract::Path<i32>,
) -> Result<Json<EmailLogResponse>> {
    // Verify the lead exists
    let _lead = state
        .lead_repository
        .get_lead_by_id(lead_id)
        .await?
        .ok_or_else(|| AppError::not_found("Lead"))?;

    let logs = state.lead_repository.get_email_logs_for_lead(lead_id).await?;

    Ok(Json(EmailLogResponse { logs }))
}

// POST /api/email/test - Test email configuration
pub async fn test_email_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    if !state.config.features.enable_email_sending {
        return Err(AppError::forbidden("Email sending is currently disabled"));
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

    let (emails_sent, _emails_failed, errors) = send_email_to_leads(
        &state.config,
        &vec![test_lead],
        test_subject,
        test_body,
    ).await?;

    if emails_sent > 0 {
        Ok(Json(serde_json::json!({
            "status": "success",
            "message": "Test email sent successfully"
        })))
    } else {
        Ok(Json(serde_json::json!({
            "status": "error",
            "message": "Failed to send test email",
            "errors": errors
        })))
    }
}