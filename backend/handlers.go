package main

import (
	"encoding/csv"
	"fmt"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

func deleteLead(c *gin.Context) {
	leadID := c.Param("id")

	var lead Lead
	if err := db.First(&lead, leadID).Error; err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Lead not found"})
		return
	}

	// Use a transaction to ensure all or nothing is deleted
	tx := db.Begin()

	// Delete associated conflicts
	if err := tx.Where("lead_id = ?", leadID).Delete(&Conflict{}).Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to delete associated conflicts"})
		return
	}

	// Delete associated validations
	if err := tx.Where("lead_id = ?", leadID).Delete(&Validation{}).Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to delete associated validations"})
		return
	}

	// Delete associated disputes
	if err := tx.Where("lead_id = ?", leadID).Delete(&Dispute{}).Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to delete associated disputes"})
		return
	}

	// Delete the lead itself
	if err := tx.Delete(&lead).Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to delete lead"})
		return
	}

	if err := tx.Commit().Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Transaction commit failed"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"message": "Lead and all associated data deleted successfully"})
}

func getLeads(c *gin.Context) {
	status := c.DefaultQuery("status", "new")
	search := c.Query("search")
	validationResult := c.Query("validation_result")
	page, _ := strconv.Atoi(c.DefaultQuery("page", "1"))
	perPage, _ := strconv.Atoi(c.DefaultQuery("per_page", "50"))

	if page < 1 {
		page = 1
	}
	if perPage < 1 || perPage > 100 {
		perPage = 50
	}

	query := db.Model(&Lead{})

	if status != "" {
		query = query.Where("status = ?", status)
	}
	if search != "" {
		query = query.Where("name ILIKE ? OR email ILIKE ?", "%"+search+"%", "%"+search+"%")
	}
	if validationResult != "" {
		subQuery := db.Model(&Validation{}).Select("lead_id").Where("result = ?", validationResult).Group("lead_id")
		query = query.Where("id IN (?)", subQuery)
	}

	var total int64
	query.Count(&total)

	offset := (page - 1) * perPage
	var leads []Lead
	query.Offset(offset).Limit(perPage).Order("created_at DESC").Find(&leads)

	totalPages := int((total + int64(perPage) - 1) / int64(perPage))

	c.JSON(http.StatusOK, LeadsResponse{
		Data: leads,
		Pagination: PaginationInfo{
			CurrentPage:  page,
			TotalPages:   totalPages,
			TotalRecords: int(total),
			PerPage:      perPage,
		},
	})
}

// FIXED: Import function that captures and uses settings
func importLeads(c *gin.Context) {
	// Capture file upload
	file, header, err := c.Request.FormFile("file")
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "File upload failed"})
		return
	}
	defer file.Close()

	// Save the file to a temporary location
	tempDir := filepath.Join(os.TempDir(), "lead_imports")
	os.MkdirAll(tempDir, os.ModePerm)
	filename := uuid.New().String() + filepath.Ext(header.Filename)
	savedPath := filepath.Join(tempDir, filename)

	if err := c.SaveUploadedFile(header, savedPath); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to save uploaded file"})
		return
	}

	// FIXED: Parse import settings from form data
	enableValidation := c.DefaultPostForm("enable_validation", "true") == "true"
	enableConflictDetection := c.DefaultPostForm("enable_conflict_detection", "true") == "true"
	autoMarkDisputed := c.DefaultPostForm("auto_mark_disputed", "true") == "true"

	// FIXED: Create job record with settings
	job := ImportJob{
		OriginalFilename:        header.Filename,
		FilePath:                savedPath,
		Status:                  "pending",
		CreatedAt:               time.Now(),
		EnableValidation:        enableValidation,
		EnableConflictDetection: enableConflictDetection,
		AutoMarkDisputed:        autoMarkDisputed,
		ConflictsDetected:       0,
		LeadsMarkedDisputed:     0,
		EmailsValidated:         0,
	}

	if err := db.Create(&job).Error; err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create import job"})
		return
	}

	// Launch the background processor
	go processImportJob(job.ID)

	// Return detailed response
	c.JSON(http.StatusAccepted, gin.H{
		"message": "File upload accepted. Processing will continue in the background.",
		"job_id":  job.ID,
		"settings": map[string]bool{
			"enable_validation":         enableValidation,
			"enable_conflict_detection": enableConflictDetection,
			"auto_mark_disputed":        autoMarkDisputed,
		},
	})
}

func getEnhancedStats(c *gin.Context) {
	stats := EnhancedStats{}
	db.Model(&Lead{}).Where("status = ?", "new").Count(&stats.NewLeads)
	db.Model(&Lead{}).Where("status = ?", "contacted").Count(&stats.ContactedLeads)

	var disputedLeadIds []uint
	db.Model(&Dispute{}).Where("status = ?", "open").Pluck("lead_id", &disputedLeadIds)
	stats.DisputedLeads = len(disputedLeadIds)

	db.Model(&Conflict{}).Where("conflict_type = ?", "same_domain").Count(&stats.DisputeStats.SameDomainConflicts)
	db.Model(&Conflict{}).Where("conflict_type = ?", "duplicate_email").Count(&stats.DisputeStats.DuplicateEmailConflicts)
	db.Model(&Conflict{}).Where("conflict_type = ?", "similar_name").Count(&stats.DisputeStats.SimilarNameConflicts)
	db.Model(&Conflict{}).Where("conflict_type = ?", "contacted_company_domain").Count(&stats.DisputeStats.ContactedDomainConflicts) // ADD THIS LINE
	stats.DisputeStats.TotalDisputed = stats.DisputedLeads

	db.Model(&Validation{}).Where("result = ?", "valid").Count(&stats.ValidationStats.ValidEmails)
	db.Model(&Validation{}).Where("result = ?", "invalid").Count(&stats.ValidationStats.InvalidEmails)
	db.Model(&Validation{}).Where("result = ?", "unknown").Count(&stats.ValidationStats.UnknownEmails)

	// Enhanced validation service statistics
	db.Model(&Validation{}).Where("service = ?", "QuickEmailVerification").Count(&stats.ValidationStats.QuickEmailUsed)
	db.Model(&Validation{}).Where("service = ?", "MyEmailVerifier").Count(&stats.ValidationStats.MyEmailVerifierUsed)
	db.Model(&Validation{}).Where("credit_status = ?", "fallback_used").Count(&stats.ValidationStats.FallbacksUsed)
	db.Model(&Validation{}).Where("credit_status = ?", "no_credits").Count(&stats.ValidationStats.NoCreditsFailures)

	c.JSON(http.StatusOK, stats)
}

// COMPLETELY REWRITTEN: Process import job with proper conflict handling
func processImportJob(jobID uint) {
	// Retrieve the job from the DB
	var job ImportJob
	if err := db.First(&job, jobID).Error; err != nil {
		log.Printf("Failed to retrieve import job %d: %v", jobID, err)
		return
	}

	log.Printf("Starting import job %d with settings: validation=%v, conflicts=%v, auto_disputed=%v",
		job.ID, job.EnableValidation, job.EnableConflictDetection, job.AutoMarkDisputed)

	// Open the saved file
	file, err := os.Open(job.FilePath)
	if err != nil {
		job.Status = "failed"
		job.Error = "Could not open saved file"
		db.Save(&job)
		log.Printf("Import job %d failed: %v", job.ID, err)
		return
	}
	defer file.Close()
	defer os.Remove(job.FilePath) // Clean up the file when done

	// Parse CSV
	reader := csv.NewReader(file)
	reader.FieldsPerRecord = -1
	records, err := reader.ReadAll()
	if err != nil || len(records) < 2 {
		job.Status = "failed"
		job.Error = "Invalid or empty CSV file"
		db.Save(&job)
		log.Printf("Import job %d failed: invalid CSV", job.ID)
		return
	}

	// Update job status to "processing"
	job.Status = "processing"
	job.TotalRows = len(records) - 1
	db.Save(&job)

	// Parse column headers
	headers := records[0]
	columnMap := make(map[string]int)
	for i, header := range headers {
		h := strings.ToLower(strings.TrimSpace(header))
		switch h {
		case "name", "full_name", "first_name", "business name":
			columnMap["name"] = i
		case "email", "email_address", "e-mail":
			columnMap["email"] = i
		case "company", "organization", "website":
			columnMap["company"] = i
		}
	}

	nameCol, nameOK := columnMap["name"]
	emailCol, emailOK := columnMap["email"]
	if !nameOK || !emailOK {
		job.Status = "failed"
		job.Error = "Could not find required 'Name' and 'Email' columns"
		db.Save(&job)
		log.Printf("Import job %d failed: missing required columns", job.ID)
		return
	}
	companyCol, companyOK := columnMap["company"]

	// Process records
	for i, record := range records[1:] {
		job.ProcessedRows = i + 1

		// Safely get data
		if len(record) <= nameCol || len(record) <= emailCol {
			continue
		}
		name := strings.TrimSpace(record[nameCol])
		email := strings.ToLower(strings.TrimSpace(record[emailCol]))
		if name == "" || email == "" {
			continue
		}

		company := ""
		if companyOK && len(record) > companyCol {
			company = strings.TrimSpace(record[companyCol])
		}

		// Check for existing lead
		var existingLead Lead
		if db.Where("email = ?", email).First(&existingLead).Error == nil {
			log.Printf("Skipping duplicate email: %s", email)
			continue // Skip duplicate
		}

		// FIXED: Create lead with proper initial status
		lead := Lead{
			Name:      name,
			Email:     email,
			Company:   company,
			Status:    "new", // Start as new, may be changed to disputed
			CreatedAt: time.Now(),
			UpdatedAt: time.Now(),
		}

		if err := db.Create(&lead).Error; err != nil {
			log.Printf("Failed to create lead %s (%s): %v", name, email, err)
			continue
		}

		log.Printf("Created lead %d: %s (%s)", lead.ID, lead.Name, lead.Email)

		// FIXED: Process the lead completely with validation and conflict detection
		hasConflicts, err := processLeadComplete(&lead, &job)
		if err != nil {
			log.Printf("Error processing lead %d: %v", lead.ID, err)
		}

		if hasConflicts {
			log.Printf("Lead %d has conflicts and was marked as disputed", lead.ID)
		}

		// Update progress periodically
		if job.ProcessedRows%10 == 0 || job.ProcessedRows == job.TotalRows {
			if err := db.Save(&job).Error; err != nil {
				log.Printf("Failed to update job progress: %v", err)
			}
			log.Printf("Import job %d progress: %d/%d rows processed", job.ID, job.ProcessedRows, job.TotalRows)
		}
	}

	// Finalize job
	now := time.Now()
	job.Status = "completed"
	job.CompletedAt = &now
	if err := db.Save(&job).Error; err != nil {
		log.Printf("Failed to finalize job: %v", err)
	}

	log.Printf("Import job %d completed: %d rows processed, %d conflicts detected, %d disputed, %d validated, %d QEV, %d MEV, %d fallbacks, %d credit failures",
		job.ID, job.ProcessedRows, job.ConflictsDetected, job.LeadsMarkedDisputed, job.EmailsValidated,
		job.QuickEmailValidations, job.MyEmailVerifierValidations, job.ValidationFallbacks, job.ValidationCreditFailures)
}

func getImportJobStatus(c *gin.Context) {
	jobID := c.Param("id")
	var job ImportJob
	if err := db.First(&job, jobID).Error; err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Job not found"})
		return
	}
	c.JSON(http.StatusOK, job)
}

func listImportJobs(c *gin.Context) {
	var jobs []ImportJob
	db.Order("created_at DESC").Limit(100).Find(&jobs)
	c.JSON(http.StatusOK, jobs)
}

func getDisputes(c *gin.Context) {
	page, _ := strconv.Atoi(c.DefaultQuery("page", "1"))
	perPage, _ := strconv.Atoi(c.DefaultQuery("per_page", "50"))

	if page < 1 {
		page = 1
	}
	if perPage < 1 || perPage > 100 {
		perPage = 50
	}

	var disputes []Dispute
	query := db.Where("status = ?", "open").Order("created_at DESC")

	var total int64
	query.Model(&Dispute{}).Count(&total)

	offset := (page - 1) * perPage
	query.Offset(offset).Limit(perPage).Preload("Lead").Find(&disputes)

	var disputeDetails []DisputeWithDetails
	for _, dispute := range disputes {
		var conflicts []Conflict
		var validations []Validation
		db.Where("lead_id = ?", dispute.LeadID).Find(&conflicts)
		db.Where("lead_id = ?", dispute.LeadID).Order("created_at DESC").Find(&validations)
		disputeDetails = append(disputeDetails, DisputeWithDetails{
			Lead:        dispute.Lead,
			Conflicts:   conflicts,
			Validations: validations,
		})
	}

	totalPages := int((total + int64(perPage) - 1) / int64(perPage))

	c.JSON(http.StatusOK, DisputesResponse{
		Data: disputeDetails,
		Pagination: PaginationInfo{
			CurrentPage:  page,
			TotalPages:   totalPages,
			TotalRecords: int(total),
			PerPage:      perPage,
		},
	})
}

func resolveDispute(c *gin.Context) {
	leadID := c.Param("id")
	var requestBody struct {
		Resolution string `json:"resolution"`
		Notes      string `json:"notes"`
	}

	if err := c.ShouldBindJSON(&requestBody); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body"})
		return
	}

	var dispute Dispute
	if err := db.Where("lead_id = ? AND status = ?", leadID, "open").First(&dispute).Error; err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Dispute not found"})
		return
	}

	var lead Lead
	if err := db.First(&lead, dispute.LeadID).Error; err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Lead not found"})
		return
	}

	switch requestBody.Resolution {
	case "accept":
		lead.Status = "new"
	case "mark_contacted":
		lead.Status = "contacted"
	case "discard":
		lead.Status = "lost"
	default:
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid resolution"})
		return
	}

	now := time.Now()
	dispute.Status = "resolved"
	dispute.Resolution = requestBody.Resolution
	dispute.Notes = requestBody.Notes
	dispute.ResolvedAt = &now

	db.Save(&lead)
	db.Save(&dispute)

	c.JSON(http.StatusOK, gin.H{"status": "resolved"})
}

func bulkResolveDisputes(c *gin.Context) {
	var requestBody struct {
		LeadIDs    []uint `json:"lead_ids"`
		Resolution string `json:"resolution"`
		Notes      string `json:"notes"`
	}
	if err := c.ShouldBindJSON(&requestBody); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body"})
		return
	}

	// Determine new lead status
	var newStatus string
	switch requestBody.Resolution {
	case "accept":
		newStatus = "new"
	case "mark_contacted":
		newStatus = "contacted"
	case "discard":
		newStatus = "lost"
	default:
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid resolution"})
		return
	}

	// Update leads
	db.Model(&Lead{}).Where("id IN ?", requestBody.LeadIDs).Update("status", newStatus)

	// Update disputes
	now := time.Now()
	result := db.Model(&Dispute{}).Where("lead_id IN ? AND status = ?", requestBody.LeadIDs, "open").Updates(Dispute{
		Status:     "resolved",
		Resolution: requestBody.Resolution,
		Notes:      requestBody.Notes,
		ResolvedAt: &now,
	})

	c.JSON(http.StatusOK, gin.H{"resolved_count": result.RowsAffected})
}

func bulkUpdateLeads(c *gin.Context) {
	var requestBody struct {
		LeadIDs []uint `json:"lead_ids"`
		Status  string `json:"status"`
	}
	if err := c.ShouldBindJSON(&requestBody); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body"})
		return
	}

	result := db.Model(&Lead{}).Where("id IN ?", requestBody.LeadIDs).Update("status", requestBody.Status)

	c.JSON(http.StatusOK, gin.H{"updated_count": result.RowsAffected})
}

func exportLeads(c *gin.Context) {
	var leads []Lead
	db.Find(&leads)

	c.Header("Content-Type", "text/csv")
	c.Header("Content-Disposition", "attachment; filename=leads_export.csv")
	writer := csv.NewWriter(c.Writer)
	defer writer.Flush()

	writer.Write([]string{"ID", "Name", "Email", "Status", "Company", "Created"})
	for _, lead := range leads {
		writer.Write([]string{
			strconv.Itoa(int(lead.ID)),
			lead.Name,
			lead.Email,
			lead.Status,
			lead.Company,
			lead.CreatedAt.Format("2006-01-02 15:04:05"),
		})
	}
}

// FIXED: Analyze conflicts function that properly marks leads as disputed
func analyzeConflicts(c *gin.Context) {
	var leads []Lead
	db.Find(&leads)

	var conflictsDetected, leadsMarkedDisputed int

	for _, lead := range leads {
		// Skip leads that are already disputed
		if lead.Status == "disputed" {
			continue
		}

		conflicts := detectConflicts(lead)
		if len(conflicts) > 0 {
			conflictsDetected += len(conflicts)

			// Mark lead as disputed
			lead.Status = "disputed"
			if err := db.Save(&lead).Error; err != nil {
				log.Printf("Failed to update lead %d status to disputed: %v", lead.ID, err)
				continue
			}

			// Create or update dispute record
			var existingDispute Dispute
			if db.Where("lead_id = ? AND status = ?", lead.ID, "open").First(&existingDispute).Error != nil {
				// No existing open dispute, create new one
				dispute := Dispute{
					LeadID:    lead.ID,
					Status:    "open",
					Notes:     fmt.Sprintf("Created during manual conflict analysis - %d conflicts found", len(conflicts)),
					CreatedAt: time.Now(),
				}
				if err := db.Create(&dispute).Error; err != nil {
					log.Printf("Failed to create dispute for lead %d: %v", lead.ID, err)
				} else {
					leadsMarkedDisputed++
				}
			}
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"conflicts_detected":    conflictsDetected,
		"leads_marked_disputed": leadsMarkedDisputed,
	})
}

func validateDisputedEmails(c *gin.Context) {
	var disputedLeadIDs []uint
	db.Model(&Dispute{}).Where("status = ?", "open").Pluck("lead_id", &disputedLeadIDs)

	var leads []Lead
	db.Where("id IN ?", disputedLeadIDs).Find(&leads)

	var validationsPerformed int
	var quickEmailUsed, myEmailVerifierUsed, fallbacksUsed, creditFailures int

	for _, lead := range leads {
		// Check if validation already exists
		var existingValidation Validation
		if db.Where("lead_id = ?", lead.ID).First(&existingValidation).Error == nil {
			log.Printf("Validation already exists for lead %d, skipping", lead.ID)
			continue
		}

		result, details, service, creditStatus := validateEmailWithTracking(lead.Email)
		validation := Validation{
			LeadID:       lead.ID,
			Result:       result,
			Details:      details,
			Service:      service,
			CreditStatus: creditStatus,
			CreatedAt:    time.Now(),
		}
		if err := db.Create(&validation).Error; err != nil {
			log.Printf("Failed to save validation for lead %d: %v", lead.ID, err)
		} else {
			validationsPerformed++

			// Track service usage
			switch service {
			case "QuickEmailVerification":
				quickEmailUsed++
			case "MyEmailVerifier":
				myEmailVerifierUsed++
			}

			switch creditStatus {
			case "fallback_used":
				fallbacksUsed++
			case "no_credits":
				creditFailures++
			}
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"validations_performed":       validationsPerformed,
		"quickemail_validations":      quickEmailUsed,
		"myemailverifier_validations": myEmailVerifierUsed,
		"fallbacks_used":              fallbacksUsed,
		"credit_failures":             creditFailures,
	})
}

func sendEmails(c *gin.Context) {
	var requestBody struct {
		LeadIDs []uint `json:"lead_ids"`
		Subject string `json:"subject"`
		Body    string `json:"body"`
	}
	if err := c.ShouldBindJSON(&requestBody); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body"})
		return
	}

	var leads []Lead
	db.Where("id IN ?", requestBody.LeadIDs).Find(&leads)
	var emailsSent, emailsFailed int

	for _, lead := range leads {
		if err := sendSMTPEmail(lead.Email, requestBody.Subject, requestBody.Body); err != nil {
			emailsFailed++
		} else {
			emailsSent++
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"emails_sent":   emailsSent,
		"emails_failed": emailsFailed,
	})
}

func sendToZapier(c *gin.Context) {
	var requestBody struct {
		LeadIDs         []uint `json:"lead_ids"`
		IntervalSeconds int    `json:"interval_seconds"`
	}
	if err := c.ShouldBindJSON(&requestBody); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body"})
		return
	}

	var leads []Lead
	db.Where("id IN ?", requestBody.LeadIDs).Find(&leads)
	var emailsSent, emailsFailed int

	for i, lead := range leads {
		subject := fmt.Sprintf("Lead %d: %s", lead.ID, lead.Name)
		body := fmt.Sprintf("Email: %s\nCompany: %s", lead.Email, lead.Company)

		if err := sendSMTPEmail(ZapierEmail, subject, body); err != nil {
			emailsFailed++
		} else {
			emailsSent++
		}

		if i < len(leads)-1 && requestBody.IntervalSeconds > 0 {
			time.Sleep(time.Duration(requestBody.IntervalSeconds) * time.Second)
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"emails_sent":   emailsSent,
		"emails_failed": emailsFailed,
	})
}
