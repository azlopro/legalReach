// Enhanced src/database/models.rs with conflict detection and validation
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
    Disputed, // New status for conflicted leads
}

impl fmt::Display for LeadStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LeadStatus::New => write!(f, "new"),
            LeadStatus::Contacted => write!(f, "contacted"),
            LeadStatus::Qualified => write!(f, "qualified"),
            LeadStatus::Converted => write!(f, "converted"),
            LeadStatus::Lost => write!(f, "lost"),
            LeadStatus::Disputed => write!(f, "disputed"),
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
            "disputed" => LeadStatus::Disputed,
            _ => LeadStatus::New, // Default to new if unknown
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    SameDomain,
    DuplicateEmail,
    SimilarName,
    // Extensible for future conflict types
}

impl fmt::Display for ConflictType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConflictType::SameDomain => write!(f, "same_domain"),
            ConflictType::DuplicateEmail => write!(f, "duplicate_email"),
            ConflictType::SimilarName => write!(f, "similar_name"),
        }
    }
}

impl From<String> for ConflictType {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "same_domain" => ConflictType::SameDomain,
            "duplicate_email" => ConflictType::DuplicateEmail,
            "similar_name" => ConflictType::SimilarName,
            _ => ConflictType::SameDomain,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationType {
    EmailActive,
    EmailSyntax,
    DomainExists,
    // Extensible for future validation types
}

impl fmt::Display for ValidationType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValidationType::EmailActive => write!(f, "email_active"),
            ValidationType::EmailSyntax => write!(f, "email_syntax"),
            ValidationType::DomainExists => write!(f, "domain_exists"),
        }
    }
}

impl From<String> for ValidationType {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "email_active" => ValidationType::EmailActive,
            "email_syntax" => ValidationType::EmailSyntax,
            "domain_exists" => ValidationType::DomainExists,
            _ => ValidationType::EmailSyntax,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationResult {
    Valid,
    Invalid,
    Unknown,
    Pending,
}

impl fmt::Display for ValidationResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValidationResult::Valid => write!(f, "valid"),
            ValidationResult::Invalid => write!(f, "invalid"),
            ValidationResult::Unknown => write!(f, "unknown"),
            ValidationResult::Pending => write!(f, "pending"),
        }
    }
}

impl From<String> for ValidationResult {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "valid" => ValidationResult::Valid,
            "invalid" => ValidationResult::Invalid,
            "unknown" => ValidationResult::Unknown,
            "pending" => ValidationResult::Pending,
            _ => ValidationResult::Unknown,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DisputeResolution {
    Accept,
    Discard,
    MarkContacted,
}

impl fmt::Display for DisputeResolution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DisputeResolution::Accept => write!(f, "accept"),
            DisputeResolution::Discard => write!(f, "discard"),
            DisputeResolution::MarkContacted => write!(f, "mark_contacted"),
        }
    }
}

impl From<String> for DisputeResolution {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "accept" => DisputeResolution::Accept,
            "discard" => DisputeResolution::Discard,
            "mark_contacted" => DisputeResolution::MarkContacted,
            _ => DisputeResolution::Accept,
        }
    }
}

// Existing Lead struct remains the same
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

// Enhanced Lead with conflict and validation information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LeadWithDetails {
    #[serde(flatten)]
    pub lead: Lead,
    pub conflicts: Vec<LeadConflict>,
    pub validations: Vec<LeadValidation>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LeadConflict {
    pub id: i32,
    pub lead_id: i32,
    pub conflict_type: ConflictType,
    pub conflict_details: String,
    pub conflicting_lead_id: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_action: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NewLeadConflict {
    pub lead_id: i32,
    pub conflict_type: ConflictType,
    pub conflict_details: String,
    pub conflicting_lead_id: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LeadValidation {
    pub id: i32,
    pub lead_id: i32,
    pub validation_type: ValidationType,
    pub result: ValidationResult,
    pub details: Option<String>,
    pub validated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NewLeadValidation {
    pub lead_id: i32,
    pub validation_type: ValidationType,
    pub result: ValidationResult,
    pub details: Option<String>,
}

// Request/Response models for dispute resolution
#[derive(Debug, Serialize, Deserialize)]
pub struct ResolveDisputeRequest {
    pub resolution: DisputeResolution,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DisputeStats {
    pub total_disputed: i64,
    pub same_domain_conflicts: i64,
    pub duplicate_email_conflicts: i64,
    pub similar_name_conflicts: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationStats {
    pub total_validations: i64,
    pub valid_emails: i64,
    pub invalid_emails: i64,
    pub pending_validations: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConflictDetectionSettings {
    pub enable_same_domain_check: bool,
    pub enable_duplicate_email_check: bool,
    pub enable_similar_name_check: bool,
    pub same_domain_threshold: i32, // Minimum number of leads from same domain to trigger conflict
    pub similar_name_threshold: f32, // Similarity threshold for name matching (0.0-1.0)
}

impl Default for ConflictDetectionSettings {
    fn default() -> Self {
        Self {
            enable_same_domain_check: true,
            enable_duplicate_email_check: true,
            enable_similar_name_check: false,
            same_domain_threshold: 3,
            similar_name_threshold: 0.8,
        }
    }
}

// Existing models remain unchanged
#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub has_conflicts: Option<bool>,
    pub validation_result: Option<ValidationResult>,
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