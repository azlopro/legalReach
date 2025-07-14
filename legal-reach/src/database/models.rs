use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LeadStatus {
    New,
    Contacted,
    Qualified,
    Converted,
    Lost,
}

impl fmt::Display for LeadStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LeadStatus::New => write!(f, "new"),
            LeadStatus::Contacted => write!(f, "contacted"),
            LeadStatus::Qualified => write!(f, "qualified"),
            LeadStatus::Converted => write!(f, "converted"),
            LeadStatus::Lost => write!(f, "lost"),
        }
    }
}

impl From<String> for LeadStatus {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "new" => LeadStatus::New,
            "contacted" => LeadStatus::Contacted,
            "qualified" => LeadStatus::Qualified,
            "converted" => LeadStatus::Converted,
            "lost" => LeadStatus::Lost,
            _ => LeadStatus::New, // Default to new if unknown
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Lead {
    pub id: i32,
    pub name: String,
    pub email: String,
    pub status: LeadStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub notes: Option<String>,
    pub source: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewLead {
    pub name: String,
    pub email: String,
    pub notes: Option<String>,
    pub source: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateLead {
    pub name: Option<String>,
    pub email: Option<String>,
    pub status: Option<LeadStatus>,
    pub notes: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailLog {
    pub id: i32,
    pub lead_id: i32,
    pub subject: String,
    pub body: String,
    pub sent_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewEmailLog {
    pub lead_id: i32,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeadFilters {
    pub status: Option<LeadStatus>,
    pub search: Option<String>,
    pub source: Option<String>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: Some(1),
            per_page: Some(50),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginationInfo {
    pub current_page: u32,
    pub per_page: u32,
    pub total_items: i64,
    pub total_pages: u32,
    pub has_next: bool,
    pub has_prev: bool,
}