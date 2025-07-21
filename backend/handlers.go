package main

import (
	"encoding/csv"
	"fmt"
	"log"
	"math/rand"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"google.golang.org/api/sheets/v4"
)

func deleteLead(c *gin.Context) {
	leadID := c.Param("id")

	var lead Lead
	if err := db.First(&lead, leadID).Error; err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Lead not found"})
		return
	}

	tx := db.Begin()

	if err := tx.Where("lead_id = ?", leadID).Delete(&Conflict{}).Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to delete associated conflicts"})
		return
	}

	if err := tx.Where("lead_id = ?", leadID).Delete(&Validation{}).Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to delete associated validations"})
		return
	}

	if err := tx.Where("lead_id = ?", leadID).Delete(&Dispute{}).Error; err != nil {
		tx.Rollback()
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to delete associated disputes"})
		return
	}

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

func validateSingleLeadEmail(c *gin.Context) {
	leadID := c.Param("id")

	var lead Lead
	if err := db.First(&lead, leadID).Error; err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Lead not found"})
		return
	}

	var existingValidation Validation
	if db.Where("lead_id = ?", lead.ID).First(&existingValidation).Error == nil {
		if existingValidation.Result == "valid" || existingValidation.Result == "invalid" {
			c.JSON(http.StatusConflict, gin.H{"error": "This lead has already been conclusively validated.", "validation": existingValidation})
			return
		}
		if err := db.Delete(&existingValidation).Error; err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to remove previous failed validation record"})
			return
		}
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
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to save validation result"})
		return
	}

	c.JSON(http.StatusOK, validation)
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

	var leadIDs []uint
	for _, l := range leads {
		leadIDs = append(leadIDs, l.ID)
	}

	var responseData []gin.H
	if len(leadIDs) > 0 {
		var validations []Validation
		db.Where("lead_id IN ?", leadIDs).Order("created_at DESC").Find(&validations)

		validationMap := make(map[uint]Validation)
		for _, v := range validations {
			if _, ok := validationMap[v.LeadID]; !ok {
				validationMap[v.LeadID] = v
			}
		}

		for _, l := range leads {
			leadMap := gin.H{
				"id":         l.ID,
				"name":       l.Name,
				"email":      l.Email,
				"status":     l.Status,
				"company":    l.Company,
				"created_at": l.CreatedAt,
				"updated_at": l.UpdatedAt,
				"validation": nil,
			}
			if v, ok := validationMap[l.ID]; ok {
				leadMap["validation"] = v
			}
			responseData = append(responseData, leadMap)
		}
	}

	totalPages := int((total + int64(perPage) - 1) / int64(perPage))

	c.JSON(http.StatusOK, gin.H{
		"data": responseData,
		"pagination": PaginationInfo{
			CurrentPage:  page,
			TotalPages:   totalPages,
			TotalRecords: int(total),
			PerPage:      perPage,
		},
	})
}

func importLeads(c *gin.Context) {
	file, header, err := c.Request.FormFile("file")
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "File upload failed"})
		return
	}
	defer file.Close()

	tempDir := filepath.Join(os.TempDir(), "lead_imports")
	os.MkdirAll(tempDir, os.ModePerm)
	filename := uuid.New().String() + filepath.Ext(header.Filename)
	savedPath := filepath.Join(tempDir, filename)

	if err := c.SaveUploadedFile(header, savedPath); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to save uploaded file"})
		return
	}

	enableValidation := c.DefaultPostForm("enable_validation", "true") == "true"
	enableConflictDetection := c.DefaultPostForm("enable_conflict_detection", "true") == "true"
	autoMarkDisputed := c.DefaultPostForm("auto_mark_disputed", "true") == "true"

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

	go processImportJob(job.ID)

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
	db.Model(&Lead{}).Where("status = ?", "pending").Count(&stats.PendingLeads) // ADD THIS LINE

	var disputedLeadIds []uint
	db.Model(&Dispute{}).Where("status = ?", "open").Pluck("lead_id", &disputedLeadIds)
	stats.DisputedLeads = len(disputedLeadIds)

	db.Model(&Conflict{}).Where("conflict_type = ?", "same_domain").Count(&stats.DisputeStats.SameDomainConflicts)
	db.Model(&Conflict{}).Where("conflict_type = ?", "duplicate_email").Count(&stats.DisputeStats.DuplicateEmailConflicts)
	db.Model(&Conflict{}).Where("conflict_type = ?", "similar_name").Count(&stats.DisputeStats.SimilarNameConflicts)
	db.Model(&Conflict{}).Where("conflict_type = ?", "contacted_company_domain").Count(&stats.DisputeStats.ContactedDomainConflicts)
	stats.DisputeStats.TotalDisputed = stats.DisputedLeads

	db.Model(&Validation{}).Where("result = ?", "valid").Count(&stats.ValidationStats.ValidEmails)
	db.Model(&Validation{}).Where("result = ?", "invalid").Count(&stats.ValidationStats.InvalidEmails)
	db.Model(&Validation{}).Where("result = ?", "unknown").Count(&stats.ValidationStats.UnknownEmails)

	db.Model(&Validation{}).Where("service = ?", "QuickEmailVerification").Count(&stats.ValidationStats.QuickEmailUsed)
	db.Model(&Validation{}).Where("service = ?", "MyEmailVerifier").Count(&stats.ValidationStats.MyEmailVerifierUsed)
	db.Model(&Validation{}).Where("credit_status = ?", "fallback_used").Count(&stats.ValidationStats.FallbacksUsed)
	db.Model(&Validation{}).Where("credit_status = ?", "no_credits").Count(&stats.ValidationStats.NoCreditsFailures)

	c.JSON(http.StatusOK, stats)
}

func processImportJob(jobID uint) {
	var job ImportJob
	if err := db.First(&job, jobID).Error; err != nil {
		log.Printf("Failed to retrieve import job %d: %v", jobID, err)
		return
	}

	log.Printf("Starting import job %d with settings: validation=%v, conflicts=%v, auto_disputed=%v",
		job.ID, job.EnableValidation, job.EnableConflictDetection, job.AutoMarkDisputed)

	file, err := os.Open(job.FilePath)
	if err != nil {
		job.Status = "failed"
		job.Error = "Could not open saved file"
		db.Save(&job)
		log.Printf("Import job %d failed: %v", job.ID, err)
		return
	}
	defer file.Close()
	defer os.Remove(job.FilePath)

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

	job.Status = "processing"
	job.TotalRows = len(records) - 1
	db.Save(&job)

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

	for i, record := range records[1:] {
		job.ProcessedRows = i + 1

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

		var existingLead Lead
		if db.Where("email = ?", email).First(&existingLead).Error == nil {
			log.Printf("Skipping duplicate email: %s", email)
			continue
		}

		lead := Lead{
			Name:      name,
			Email:     email,
			Company:   company,
			Status:    "new",
			CreatedAt: time.Now(),
			UpdatedAt: time.Now(),
		}

		if err := db.Create(&lead).Error; err != nil {
			log.Printf("Failed to create lead %s (%s): %v", name, email, err)
			continue
		}

		log.Printf("Created lead %d: %s (%s)", lead.ID, lead.Name, lead.Email)

		hasConflicts, err := processLeadComplete(&lead, &job)
		if err != nil {
			log.Printf("Error processing lead %d: %v", lead.ID, err)
		}

		if hasConflicts {
			log.Printf("Lead %d has conflicts and was marked as disputed", lead.ID)
		}

		if job.ProcessedRows%10 == 0 || job.ProcessedRows == job.TotalRows {
			if err := db.Save(&job).Error; err != nil {
				log.Printf("Failed to update job progress: %v", err)
			}
			log.Printf("Import job %d progress: %d/%d rows processed", job.ID, job.ProcessedRows, job.TotalRows)
		}
	}

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

	db.Model(&Lead{}).Where("id IN ?", requestBody.LeadIDs).Update("status", newStatus)

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

func analyzeConflicts(c *gin.Context) {
	var leads []Lead
	db.Find(&leads)

	var conflictsDetected, leadsMarkedDisputed int

	for _, lead := range leads {
		if lead.Status == "disputed" {
			continue
		}

		conflicts := detectConflicts(lead)
		if len(conflicts) > 0 {
			conflictsDetected += len(conflicts)

			lead.Status = "disputed"
			if err := db.Save(&lead).Error; err != nil {
				log.Printf("Failed to update lead %d status to disputed: %v", lead.ID, err)
				continue
			}

			var existingDispute Dispute
			if db.Where("lead_id = ? AND status = ?", lead.ID, "open").First(&existingDispute).Error != nil {
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

func findRowByLeadID(srv *sheets.Service, sheetID, sheetName string, leadID uint) (int, error) {
	readRange := fmt.Sprintf("%s!A:A", sheetName)
	resp, err := srv.Spreadsheets.Values.Get(sheetID, readRange).Do()
	if err != nil {
		return -1, fmt.Errorf("unable to retrieve data from sheet: %v", err)
	}

	if len(resp.Values) == 0 {
		return -1, fmt.Errorf("sheet is empty")
	}

	leadIDStr := strconv.FormatUint(uint64(leadID), 10)

	for i, row := range resp.Values {
		if len(row) > 0 && row[0] == leadIDStr {
			return i + 1, nil
		}
	}

	return -1, fmt.Errorf("lead ID %d not found in sheet", leadID)
}

func getLeadStatusFromSheet(srv *sheets.Service, sheetID, sheetName string, leadID uint) (string, error) {
	rowNum, err := findRowByLeadID(srv, sheetID, sheetName, leadID)
	if err != nil {
		return "", err
	}

	readRange := fmt.Sprintf("%s!B%d", sheetName, rowNum)
	resp, err := srv.Spreadsheets.Values.Get(sheetID, readRange).Do()
	if err != nil {
		return "", fmt.Errorf("unable to retrieve status for lead %d (row %d): %v", leadID, rowNum, err)
	}

	if len(resp.Values) == 0 || len(resp.Values[0]) == 0 {
		return "0", nil
	}

	return fmt.Sprintf("%v", resp.Values[0][0]), nil
}

// MODIFIED: This function now correctly manages the 'pending' state.
func sendToZapier(c *gin.Context) {
	var requestBody struct {
		LeadIDs         []uint `json:"lead_ids"`
		IntervalSeconds int    `json:"interval_seconds"`
	}
	if err := c.ShouldBindJSON(&requestBody); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body"})
		return
	}

	if GoogleSheetCredentials == "your-credentials.json" || GoogleSheetID == "your-spreadsheet-id-here" {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": gin.H{"message": "Google Sheets API is not configured on the server."}})
		return
	}

	sheetsService, err := getSheetsService()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": gin.H{"message": fmt.Sprintf("Failed to initialize Google Sheets service: %v", err)}})
		return
	}

	// --- CHANGE: Update lead status to 'pending' in a transaction first ---
	if err := db.Model(&Lead{}).Where("id IN ?", requestBody.LeadIDs).Update("status", "pending").Error; err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to update lead status to pending"})
		return
	}
	// --- END CHANGE ---

	var leads []Lead
	db.Where("id IN ?", requestBody.LeadIDs).Find(&leads)
	var emailsSent, emailsFailed int

	for i, lead := range leads {
		subject := fmt.Sprintf("Lead %d: %s", lead.ID, lead.Name)
		body := fmt.Sprintf("Email: %s\nCompany: %s", lead.Email, lead.Company)

		if err := sendSMTPEmail(ZapierEmail, subject, body); err != nil {
			emailsFailed++
			log.Printf("Failed to send email to Zapier for lead %d: %v", lead.ID, err)
			// --- CHANGE: Revert status on immediate failure ---
			db.Model(&lead).Update("status", "new")
			// --- END CHANGE ---
		} else {
			emailsSent++
			log.Printf("Successfully sent email to Zapier for lead %d", lead.ID)

			go func(currentLead Lead) {
				values := []interface{}{currentLead.ID, 0}
				if err := appendToSheet(sheetsService, GoogleSheetID, "Sheet1", values); err != nil {
					log.Printf("Failed to append to sheet for lead %d: %v", currentLead.ID, err)
					// --- CHANGE: Revert status if sheet write fails ---
					db.Model(&currentLead).Update("status", "new")
					// --- END CHANGE ---
					return
				}

				go func() {
					const maxRetries = 12
					const initialDelay = 5 * time.Second
					const maxDelay = 60 * time.Second
					delay := initialDelay

					for attempt := 0; attempt < maxRetries; attempt++ {
						time.Sleep(delay)
						status, err := getLeadStatusFromSheet(sheetsService, GoogleSheetID, "Sheet1", currentLead.ID)
						if err != nil {
							log.Printf("Polling attempt %d for lead %d: Waiting for Zapier... (%v)", attempt+1, currentLead.ID, err)
							delay *= 2
							if delay > maxDelay {
								delay = maxDelay
							}
							continue
						}

						// --- CHANGE: More robust status handling ---
						shouldExitPolling := false
						switch status {
						case "1":
							log.Printf("SUCCESS: Zapier confirmed processing for lead %d. Updating status to 'contacted'.", currentLead.ID)
							db.Model(&currentLead).Update("status", "contacted")
							shouldExitPolling = true
						case "2", "3", "4":
							log.Printf("ERROR/WARNING: Zapier reported status '%s' for lead %d. Reverting to 'new'.", status, currentLead.ID)
							db.Model(&currentLead).Update("status", "new")
							shouldExitPolling = true
						default:
							log.Printf("Polling attempt %d for lead %d: Status is '%s', waiting...", attempt+1, currentLead.ID, status)
						}

						if shouldExitPolling {
							return // Exit the polling loop.
						}
						// --- END CHANGE ---

						delay *= 2
						if delay > maxDelay {
							delay = maxDelay
						}
					}

					// --- CHANGE: Timeout handling ---
					log.Printf("TIMEOUT: Polling for lead %d stopped. Reverting to 'new'.", currentLead.ID)
					db.Model(&currentLead).Update("status", "new")
					// --- END CHANGE ---
				}()
			}(lead)
		}

		if i < len(leads)-1 && requestBody.IntervalSeconds > 0 {
			jitterMagnitude := int(float64(requestBody.IntervalSeconds) * 0.30)
			if jitterMagnitude == 0 {
				jitterMagnitude = 1
			}
			randomJitter := rand.Intn(jitterMagnitude*2) - jitterMagnitude
			sleepDuration := time.Duration(requestBody.IntervalSeconds+randomJitter) * time.Second
			if sleepDuration < time.Second {
				sleepDuration = time.Second
			}
			log.Printf("Waiting for next send: Base Interval: %ds, Jitter: %ds, Final Wait: %v", requestBody.IntervalSeconds, randomJitter, sleepDuration)
			time.Sleep(sleepDuration)
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"message":       "Zapier process initiated. Leads moved to 'Pending' status.",
		"emails_sent":   emailsSent,
		"emails_failed": emailsFailed,
	})
}
