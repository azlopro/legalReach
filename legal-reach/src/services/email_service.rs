use lettre::{
    message::{header::ContentType, Mailbox},
    transport::smtp::{authentication::Credentials, PoolConfig},
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use crate::{
    config::Settings,
    database::models::Lead,
    errors::{AppError, Result},
};

// Zapier webhook email address
const ZAPIER_EMAIL: &str = "odf86lbl@robot.zapier.com";

pub async fn send_email_to_leads(
    config: &Settings,
    leads: &[Lead],
    subject: &str,
    body: &str,
) -> Result<(usize, usize, Vec<String>)> {
    if config.email.mock_mode {
        return send_mock_emails(leads, subject, body).await;
    }

    send_real_emails(config, leads, subject, body).await
}

pub async fn send_leads_to_zapier(
    config: &Settings,
    leads: &[Lead],
    interval_seconds: u64,
) -> Result<(usize, usize, Vec<String>)> {
    if config.email.mock_mode {
        return send_mock_zapier_emails(leads, interval_seconds).await;
    }

    send_real_zapier_emails(config, leads, interval_seconds).await
}

async fn send_real_zapier_emails(
    config: &Settings,
    leads: &[Lead],
    interval_seconds: u64,
) -> Result<(usize, usize, Vec<String>)> {
    // Create SMTP transport
    let creds = Credentials::new(
        config.email.smtp_username.clone(),
        config.email.smtp_password.clone(),
    );

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.email.smtp_host)
        .map_err(|e| AppError::SmtpTransport(e))?
        .port(config.email.smtp_port)
        .credentials(creds)
        .pool_config(PoolConfig::new().max_size(5))
        .build();

    let from_mailbox: Mailbox = format!("{} <{}>", config.email.from_name, config.email.from_email)
        .parse()
        .map_err(|e| AppError::internal(format!("Invalid from email address: {}", e)))?;

    let zapier_mailbox: Mailbox = ZAPIER_EMAIL
        .parse()
        .map_err(|e| AppError::internal(format!("Invalid Zapier email address: {}", e)))?;

    let mut emails_sent = 0;
    let mut emails_failed = 0;
    let mut errors = Vec::new();

    tracing::info!("Starting bulk send to Zapier for {} leads with {}s intervals", leads.len(), interval_seconds);

    for (index, lead) in leads.iter().enumerate() {
        // Create email with lead ID as subject and lead email as body
        let subject = lead.id.to_string();
        let body = lead.email.clone();

        match send_single_zapier_email(&mailer, &from_mailbox, &zapier_mailbox, &subject, &body, lead.id).await {
            Ok(_) => {
                emails_sent += 1;
                tracing::info!("Zapier email sent successfully for lead {} ({}/{})", lead.id, index + 1, leads.len());
            }
            Err(e) => {
                emails_failed += 1;
                let error_msg = format!("Failed to send Zapier email for lead {}: {}", lead.id, e);
                errors.push(error_msg.clone());
                tracing::error!("{}", error_msg);
            }
        }

        // Add interval delay between emails (except for the last one)
        if index < leads.len() - 1 {
            tracing::debug!("Waiting {}s before sending next email...", interval_seconds);
            tokio::time::sleep(tokio::time::Duration::from_secs(interval_seconds)).await;
        }
    }

    tracing::info!("Bulk send to Zapier completed: {} sent, {} failed", emails_sent, emails_failed);

    Ok((emails_sent, emails_failed, errors))
}

async fn send_single_zapier_email(
    mailer: &AsyncSmtpTransport<Tokio1Executor>,
    from_mailbox: &Mailbox,
    to_mailbox: &Mailbox,
    subject: &str,
    body: &str,
    lead_id: i32,
) -> Result<()> {
    let email = Message::builder()
        .from(from_mailbox.clone())
        .to(to_mailbox.clone())
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| AppError::internal(format!("Failed to build Zapier email for lead {}: {}", lead_id, e)))?;

    mailer
        .send(email)
        .await
        .map_err(|e| AppError::SmtpTransport(e))?;

    Ok(())
}

async fn send_mock_zapier_emails(
    leads: &[Lead],
    interval_seconds: u64,
) -> Result<(usize, usize, Vec<String>)> {
    tracing::info!("Mock Zapier email mode: simulating bulk send for {} leads with {}s intervals", leads.len(), interval_seconds);
    
    let mut emails_sent = 0;
    let mut emails_failed = 0;
    let mut errors = Vec::new();

    for (index, lead) in leads.iter().enumerate() {
        // Simulate some processing time
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Simulate a 98% success rate for Zapier emails
        if rand::random::<f32>() < 0.98 {
            emails_sent += 1;
            tracing::info!(
                "MOCK ZAPIER EMAIL SENT ({}/{}):\nTo: {}\nSubject: {}\nBody: {}\n",
                index + 1,
                leads.len(),
                ZAPIER_EMAIL,
                lead.id,
                lead.email
            );
        } else {
            emails_failed += 1;
            let error_msg = format!("Mock failure: randomly failed to send Zapier email for lead {}", lead.id);
            errors.push(error_msg.clone());
            tracing::warn!("{}", error_msg);
        }

        // Add interval delay between emails (except for the last one)
        if index < leads.len() - 1 {
            tracing::debug!("Mock waiting {}s before sending next email...", interval_seconds);
            tokio::time::sleep(tokio::time::Duration::from_secs(interval_seconds)).await;
        }
    }

    Ok((emails_sent, emails_failed, errors))
}

async fn send_real_emails(
    config: &Settings,
    leads: &[Lead],
    subject: &str,
    body: &str,
) -> Result<(usize, usize, Vec<String>)> {
    // Create SMTP transport
    let creds = Credentials::new(
        config.email.smtp_username.clone(),
        config.email.smtp_password.clone(),
    );

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.email.smtp_host)
        .map_err(|e| AppError::SmtpTransport(e))?
        .port(config.email.smtp_port)
        .credentials(creds)
        .pool_config(PoolConfig::new().max_size(5))
        .build();

    let from_mailbox: Mailbox = format!("{} <{}>", config.email.from_name, config.email.from_email)
        .parse()
        .map_err(|e| AppError::internal(format!("Invalid from email address: {}", e)))?;

    let mut emails_sent = 0;
    let mut emails_failed = 0;
    let mut errors = Vec::new();

    for lead in leads {
        match send_single_email(&mailer, &from_mailbox, lead, subject, body).await {
            Ok(_) => {
                emails_sent += 1;
                tracing::info!("Email sent successfully to {}", lead.email);
            }
            Err(e) => {
                emails_failed += 1;
                let error_msg = format!("Failed to send email to {}: {}", lead.email, e);
                errors.push(error_msg.clone());
                tracing::error!("{}", error_msg);
            }
        }

        // Add a small delay between emails to avoid overwhelming the SMTP server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Ok((emails_sent, emails_failed, errors))
}

async fn send_single_email(
    mailer: &AsyncSmtpTransport<Tokio1Executor>,
    from_mailbox: &Mailbox,
    lead: &Lead,
    subject: &str,
    body: &str,
) -> Result<()> {
    let to_mailbox: Mailbox = format!("{} <{}>", lead.name, lead.email)
        .parse()
        .map_err(|e| AppError::internal(format!("Invalid recipient email address: {}", e)))?;

    // Personalize the email body
    let personalized_body = personalize_email_body(body, lead);

    let email = Message::builder()
        .from(from_mailbox.clone())
        .to(to_mailbox)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(personalized_body)
        .map_err(|e| AppError::internal(format!("Failed to build email: {}", e)))?;

    mailer
        .send(email)
        .await
        .map_err(|e| AppError::SmtpTransport(e))?;

    Ok(())
}

async fn send_mock_emails(
    leads: &[Lead],
    subject: &str,
    body: &str,
) -> Result<(usize, usize, Vec<String>)> {
    tracing::info!("Mock email mode: simulating email sending");
    
    let mut emails_sent = 0;
    let mut emails_failed = 0;
    let mut errors = Vec::new();

    for lead in leads {
        // Simulate some processing time
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Simulate a 95% success rate
        if rand::random::<f32>() < 0.95 {
            emails_sent += 1;
            tracing::info!(
                "MOCK EMAIL SENT:\nTo: {} <{}>\nSubject: {}\nBody: {}\n",
                lead.name,
                lead.email,
                subject,
                personalize_email_body(body, lead)
            );
        } else {
            emails_failed += 1;
            let error_msg = format!("Mock failure: randomly failed to send to {}", lead.email);
            errors.push(error_msg.clone());
            tracing::warn!("{}", error_msg);
        }
    }

    Ok((emails_sent, emails_failed, errors))
}

fn personalize_email_body(body: &str, lead: &Lead) -> String {
    body.replace("{{name}}", &lead.name)
        .replace("{{email}}", &lead.email)
        .replace("{{company}}", lead.company.as_deref().unwrap_or("your company"))
}

pub async fn test_email_connection(config: &Settings) -> Result<()> {
    if config.email.mock_mode {
        tracing::info!("Email is in mock mode - skipping real connection test");
        return Ok(());
    }

    let creds = Credentials::new(
        config.email.smtp_username.clone(),
        config.email.smtp_password.clone(),
    );

    let mailer: AsyncSmtpTransport<Tokio1Executor> = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.email.smtp_host)
        .map_err(|e| AppError::SmtpTransport(e))?
        .port(config.email.smtp_port)
        .credentials(creds)
        .timeout(Some(std::time::Duration::from_secs(10)))
        .build();

    // Test the connection by checking if we can connect
    mailer
        .test_connection()
        .await
        .map_err(|e| AppError::internal(format!("Email connection test failed: {}", e)))?;

    tracing::info!("Email connection test successful");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::LeadStatus;

    #[test]
    fn test_personalize_email_body() {
        let lead = Lead {
            id: 1,
            name: "John Doe".to_string(),
            email: "john@example.com".to_string(),
            status: LeadStatus::New,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            notes: None,
            source: None,
            phone: None,
            company: Some("ACME Corp".to_string()),
        };

        let body = "Hello {{name}}, thank you for your interest from {{company}}.";
        let result = personalize_email_body(body, &lead);
        
        assert_eq!(result, "Hello John Doe, thank you for your interest from ACME Corp.");
    }

    #[test]
    fn test_personalize_email_body_no_company() {
        let lead = Lead {
            id: 1,
            name: "John Doe".to_string(),
            email: "john@example.com".to_string(),
            status: LeadStatus::New,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            notes: None,
            source: None,
            phone: None,
            company: None,
        };

        let body = "Hello {{name}} from {{company}}.";
        let result = personalize_email_body(body, &lead);
        
        assert_eq!(result, "Hello John Doe from your company.");
    }

    #[tokio::test]
    async fn test_send_mock_emails() {
        let leads = vec![
            Lead {
                id: 1,
                name: "John Doe".to_string(),
                email: "john@example.com".to_string(),
                status: LeadStatus::New,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                notes: None,
                source: None,
                phone: None,
                company: None,
            }
        ];

        let (sent, failed, errors) = send_mock_emails(&leads, "Test Subject", "Test Body").await.unwrap();
        
        assert_eq!(sent + failed, 1);
        if failed > 0 {
            assert!(!errors.is_empty());
        }
    }

    #[tokio::test]
    async fn test_send_mock_zapier_emails() {
        let leads = vec![
            Lead {
                id: 123,
                name: "John Doe".to_string(),
                email: "john@example.com".to_string(),
                status: LeadStatus::New,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                notes: None,
                source: None,
                phone: None,
                company: None,
            }
        ];

        let (sent, failed, errors) = send_mock_zapier_emails(&leads, 1).await.unwrap();
        
        assert_eq!(sent + failed, 1);
        if failed > 0 {
            assert!(!errors.is_empty());
        }
    }
}