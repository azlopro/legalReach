use bytes::Bytes;
use csv::{Reader, Writer};
use regex::Regex;
use std::io::Cursor;
use crate::{
    database::models::{Lead, NewLead},
    errors::{AppError, Result},
};

pub fn parse_csv_from_multipart(data: &Bytes) -> Result<Vec<NewLead>> {
    let cursor = Cursor::new(data);
    let mut reader = Reader::from_reader(cursor);
    let mut leads = Vec::new();
    let email_regex = Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").unwrap();

    // Get headers and validate required fields
    let headers = reader.headers()?.clone();
    let name_index = find_header_index(&headers, &["name", "full_name", "fullname", "lead_name"])?;
    let email_index = find_header_index(&headers, &["email", "email_address", "e_mail"])?;
    
    // Optional fields
    let notes_index = find_optional_header_index(&headers, &["notes", "note", "comment", "comments"]);
    let source_index = find_optional_header_index(&headers, &["source", "lead_source", "origin"]);
    let phone_index = find_optional_header_index(&headers, &["phone", "phone_number", "telephone", "mobile"]);
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
            lead.notes.as_deref().unwrap_or(""),
            lead.source.as_deref().unwrap_or(""),
            lead.phone.as_deref().unwrap_or(""),
            lead.company.as_deref().unwrap_or(""),
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
        if candidates.iter().any(|&candidate| header_lower == candidate) {
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
        if candidates.iter().any(|&candidate| header_lower == candidate) {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_csv_basic() {
        let csv_data = "name,email\nJohn Doe,john@example.com\nJane Smith,jane@example.com";
        let bytes = Bytes::from(csv_data);
        
        let result = parse_csv_from_multipart(&bytes).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "John Doe");
        assert_eq!(result[0].email, "john@example.com");
    }

    #[test]
    fn test_parse_csv_with_optional_fields() {
        let csv_data = "name,email,phone,company\nJohn Doe,john@example.com,123-456-7890,ACME Corp";
        let bytes = Bytes::from(csv_data);
        
        let result = parse_csv_from_multipart(&bytes).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].phone, Some("123-456-7890".to_string()));
        assert_eq!(result[0].company, Some("ACME Corp".to_string()));
    }

    #[test]
    fn test_parse_csv_invalid_email() {
        let csv_data = "name,email\nJohn Doe,invalid-email";
        let bytes = Bytes::from(csv_data);
        
        let result = parse_csv_from_multipart(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_csv() {
        let leads = vec![
            Lead {
                id: 1,
                name: "John Doe".to_string(),
                email: "john@example.com".to_string(),
                status: crate::database::models::LeadStatus::New,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                notes: None,
                source: Some("test".to_string()),
                phone: None,
                company: None,
            }
        ];

        let result = generate_csv(&leads).unwrap();
        assert!(result.contains("John Doe"));
        assert!(result.contains("john@example.com"));
        assert!(result.contains("new"));
    }
}