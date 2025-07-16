package main

import "time"

// Database Models
type Lead struct {
	ID        uint      `json:"id" gorm:"primaryKey"`
	Name      string    `json:"name" gorm:"not null"`
	Email     string    `json:"email" gorm:"uniqueIndex;not null"`
	Status    string    `json:"status" gorm:"default:'new'"`
	Company   string    `json:"company"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

type Conflict struct {
	ID              uint      `json:"id" gorm:"primaryKey"`
	LeadID          uint      `json:"lead_id"`
	ConflictType    string    `json:"conflict_type"`
	ConflictDetails string    `json:"conflict_details"`
	CreatedAt       time.Time `json:"created_at"`
	Lead            Lead      `json:"lead" gorm:"foreignKey:LeadID"`
}

type Validation struct {
	ID        uint      `json:"id" gorm:"primaryKey"`
	LeadID    uint      `json:"lead_id"`
	Result    string    `json:"result"` // valid, invalid, unknown
	Details   string    `json:"details"`
	CreatedAt time.Time `json:"created_at"`
	Lead      Lead      `json:"lead" gorm:"foreignKey:LeadID"`
}

type Dispute struct {
	ID         uint       `json:"id" gorm:"primaryKey"`
	LeadID     uint       `json:"lead_id"`
	Status     string     `json:"status" gorm:"default:'open'"`
	CreatedAt  time.Time  `json:"created_at"`
	ResolvedAt *time.Time `json:"resolved_at"`
	Resolution string     `json:"resolution"`
	Notes      string     `json:"notes"`
	Lead       Lead       `json:"lead" gorm:"foreignKey:LeadID"`
}

// Response DTOs
type PaginationInfo struct {
	CurrentPage  int `json:"current_page"`
	TotalPages   int `json:"total_pages"`
	TotalRecords int `json:"total_records"`
	PerPage      int `json:"per_page"`
}

type LeadsResponse struct {
	Data       []Lead         `json:"data"`
	Pagination PaginationInfo `json:"pagination"`
}

type DisputeWithDetails struct {
	Lead        Lead         `json:"lead"`
	Conflicts   []Conflict   `json:"conflicts"`
	Validations []Validation `json:"validations"`
}

type DisputesResponse struct {
	Data       []DisputeWithDetails `json:"data"`
	Pagination PaginationInfo       `json:"pagination"`
}

type EnhancedStats struct {
	NewLeads       int64 `json:"new_leads"`
	ContactedLeads int64 `json:"contacted_leads"`
	DisputedLeads  int   `json:"disputed_leads"`
	DisputeStats   struct {
		TotalDisputed           int   `json:"total_disputed"`
		SameDomainConflicts     int64 `json:"same_domain_conflicts"`
		DuplicateEmailConflicts int64 `json:"duplicate_email_conflicts"`
		SimilarNameConflicts    int64 `json:"similar_name_conflicts"`
	} `json:"dispute_stats"`
	ValidationStats struct {
		ValidEmails   int64 `json:"valid_emails"`
		InvalidEmails int64 `json:"invalid_emails"`
		UnknownEmails int64 `json:"unknown_emails"`
	} `json:"validation_stats"`
}

// FIXED: Import Job with Settings
type ImportJob struct {
	ID               uint       `json:"id" gorm:"primaryKey"`
	OriginalFilename string     `json:"original_filename"`
	FilePath         string     `json:"-"`      // Hide from JSON responses
	Status           string     `json:"status"` // e.g., "pending", "processing", "completed", "failed"
	TotalRows        int        `json:"total_rows"`
	ProcessedRows    int        `json:"processed_rows"`
	Error            string     `json:"error,omitempty"`
	CreatedAt        time.Time  `json:"created_at"`
	CompletedAt      *time.Time `json:"completed_at,omitempty"`

	// NEW: Import Settings
	EnableValidation        bool `json:"enable_validation" gorm:"default:true"`
	EnableConflictDetection bool `json:"enable_conflict_detection" gorm:"default:true"`
	AutoMarkDisputed        bool `json:"auto_mark_disputed" gorm:"default:true"`

	// NEW: Import Results
	ConflictsDetected   int `json:"conflicts_detected" gorm:"default:0"`
	LeadsMarkedDisputed int `json:"leads_marked_disputed" gorm:"default:0"`
	EmailsValidated     int `json:"emails_validated" gorm:"default:0"`
}
