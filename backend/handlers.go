package main

import (
	"encoding/csv"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

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
func importLeads(c *gin.Context) {
	// CORRECTED: Capture all 3 return values (file, header, err)
	file, header, err := c.Request.FormFile("file")
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "File upload failed"})
		return
	}
	defer file.Close()

	// Save the file to a temporary location
	tempDir := filepath.Join(os.TempDir(), "lead_imports")
	os.MkdirAll(tempDir, os.ModePerm)
	// Create a unique filename to avoid conflicts, using the original extension
	filename := uuid.New().String() + filepath.Ext(header.Filename)
	savedPath := filepath.Join(tempDir, filename)

	if err := c.SaveUploadedFile(header, savedPath); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to save uploaded file"})
		return
	}

	// Create the job record in the database
	job := ImportJob{
		OriginalFilename: header.Filename, // Use the filename from the header
		FilePath:         savedPath,
		Status:           "pending",
		CreatedAt:        time.Now(),
	}
	if err := db.Create(&job).Error; err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create import job"})
		return
	}

	// Launch the background processor in a new goroutine
	go processImportJob(job.ID)

	// Return 202 Accepted to the user immediately
	c.JSON(http.StatusAccepted, gin.H{
		"message": "File upload accepted. Processing will continue in the background.",
		"job_id":  job.ID,
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
	stats.DisputeStats.TotalDisputed = stats.DisputedLeads

	db.Model(&Validation{}).Where("result = ?", "valid").Count(&stats.ValidationStats.ValidEmails)
	db.Model(&Validation{}).Where("result = ?", "invalid").Count(&stats.ValidationStats.InvalidEmails)
	db.Model(&Validation{}).Where("result = ?", "unknown").Count(&stats.ValidationStats.UnknownEmails)

	c.JSON(http.StatusOK, stats)
}
func processImportJob(jobID uint) {
	// Retrieve the job from the DB
	var job ImportJob
	db.First(&job, jobID)

	// Open the saved file
	file, err := os.Open(job.FilePath)
	if err != nil {
		job.Status = "failed"
		job.Error = "Could not open saved file"
		db.Save(&job)
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
		return
	}

	// Update job status to "processing"
	job.Status = "processing"
	job.TotalRows = len(records) - 1
	db.Save(&job)

	// --- This is the core logic from the old importLeads function ---
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
			continue // Skip duplicate
		}

		// Create lead and perform validation/conflict detection
		lead := Lead{Name: name, Email: email, Company: company, Status: "new"}
		db.Create(&lead)

		// NOTE: These are synchronous calls. The job will be slower but more accurate.
		validateEmail(lead.Email)
		detectConflicts(lead)

		// Update progress periodically to avoid too many DB writes
		if job.ProcessedRows%25 == 0 || job.ProcessedRows == job.TotalRows {
			db.Save(&job)
		}
	}

	// Finalize job
	now := time.Now()
	job.Status = "completed"
	job.CompletedAt = &now
	db.Save(&job)
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

func analyzeConflicts(c *gin.Context) {
	var leads []Lead
	db.Find(&leads)

	var conflictsDetected, leadsMarkedDisputed int

	for _, lead := range leads {
		conflicts := detectConflicts(lead)
		if len(conflicts) > 0 {
			hasNewConflicts := false
			for _, conflict := range conflicts {
				var existingConflict int64
				db.Model(&Conflict{}).Where("lead_id = ? AND conflict_type = ?", lead.ID, conflict.ConflictType).Count(&existingConflict)
				if existingConflict == 0 {
					db.Create(&conflict)
					conflictsDetected++
					hasNewConflicts = true
				}
			}

			if hasNewConflicts && lead.Status != "disputed" {
				lead.Status = "disputed"
				db.Save(&lead)
				db.Create(&Dispute{LeadID: lead.ID, Status: "open", Notes: "Marked during conflict analysis"})
				leadsMarkedDisputed++
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

	for _, lead := range leads {
		result, details := validateEmail(lead.Email)
		db.Create(&Validation{
			LeadID:  lead.ID,
			Result:  result,
			Details: details,
		})
		validationsPerformed++
	}

	c.JSON(http.StatusOK, gin.H{"validations_performed": validationsPerformed})
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
