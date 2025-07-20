// services.go

package main

import (
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/smtp"
	"regexp"
	"strings"
	"time"

	"github.com/texttheater/golang-levenshtein/levenshtein"
)

// --- Structs for API Responses ---

type quickEmailResponse struct {
	Result string `json:"result"` // valid, invalid, unknown
	Reason string `json:"reason"`
}

type myEmailVerifierResponse struct {
	Status string `json:"status"` // success, error
	Result struct {
		Email      string `json:"email"`
		Status     string `json:"status"` // valid, invalid, unknown, risky
		Reason     string `json:"reason"`
		DidYouMean string `json:"didyoumean"`
	} `json:"result"`
	Credits struct {
		Used      int `json:"used"`
		Available int `json:"available"`
	} `json:"credits"`
	Error struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

type abuseIPDBResponse struct {
	Data struct {
		AbuseConfidenceScore int    `json:"abuseConfidenceScore"`
		Domain               string `json:"domain"`
	} `json:"data"`
}

// --- Validation Status Constants ---
const (
	ValidationValid   = "valid"
	ValidationInvalid = "invalid"
	ValidationUnknown = "unknown"
)

// --- Main Validation Function ---

// validateEmailWithTracking orchestrates multiple validation checks with fallback logic and tracking.
func validateEmailWithTracking(email string) (string, string, string, string) {
	// 1. Basic format check
	emailRegex := regexp.MustCompile(`^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`)
	if !emailRegex.MatchString(email) {
		return ValidationInvalid, "Invalid email format", "basic_validation", "success"
	}

	var usedService string
	var creditStatus string = "success"

	// 2. Try QuickEmailVerification first
	qevResult, qevDetails, qevErr := verifyWithQuickEmail(email)

	// Check if QuickEmailVerification failed due to no credits or other issues
	if qevErr != nil {
		log.Printf("QuickEmailVerification API error: %v", qevErr)

		// Check if this is a credit exhaustion error
		if isCreditsExhaustedError(qevErr.Error()) {
			log.Printf("QuickEmailVerification credits exhausted, trying MyEmailVerifier...")
			creditStatus = "fallback_used"

			// 3. Fallback to MyEmailVerifier
			mevResult, mevDetails, mevErr := verifyWithMyEmailVerifier(email)
			if mevErr != nil {
				log.Printf("MyEmailVerifier API error: %v", mevErr)

				if isCreditsExhaustedError(mevErr.Error()) {
					return ValidationUnknown, "No verification credits available on either service", "none", "no_credits"
				}

				return ValidationUnknown, fmt.Sprintf("Both verification services failed: QEV: %v, MEV: %v", qevErr, mevErr), "both_failed", "error"
			}

			// Use MyEmailVerifier result
			qevResult = mevResult
			qevDetails = fmt.Sprintf("MyEmailVerifier (fallback): %s", mevDetails)
			usedService = "MyEmailVerifier"
		} else {
			// Other error, mark as unknown
			qevResult = ValidationUnknown
			qevDetails = fmt.Sprintf("Verification service error: %s", qevErr.Error())
			usedService = "QuickEmailVerification"
			creditStatus = "error"
		}
	} else {
		// QuickEmailVerification succeeded
		usedService = "QuickEmailVerification"
		qevDetails = fmt.Sprintf("QuickEmailVerification: %s", qevDetails)
	}

	// If the email is definitively invalid, stop here.
	if qevResult == ValidationInvalid {
		return ValidationInvalid, fmt.Sprintf("Verification failed: %s", qevDetails), usedService, creditStatus
	}

	// 4. AbuseIPDB domain reputation check
	isSuspicious, abuseDetails, err := checkDomainWithAbuseIPDB(email)
	if err != nil {
		log.Printf("AbuseIPDB API error: %v", err)
		abuseDetails = "Domain reputation check failed"
	}

	// 5. Combine results and make a final decision
	finalDetails := fmt.Sprintf("Email Service: %s. Domain Reputation: %s.", qevDetails, abuseDetails)

	if isSuspicious {
		// If the domain is suspicious, we downgrade the status to "unknown" for manual review.
		return ValidationUnknown, finalDetails, usedService, creditStatus
	}

	// Otherwise, we trust the result from the email verification service.
	return qevResult, finalDetails, usedService, creditStatus
}

// --- API Helper Functions ---

// verifyWithQuickEmail checks an email address using the QuickEmailVerification API.
func verifyWithQuickEmail(email string) (string, string, error) {
	if QuickEmailKey == "" {
		return ValidationUnknown, "QuickEmailVerification API key not configured", fmt.Errorf("no API key configured")
	}

	client := &http.Client{Timeout: 30 * time.Second}
	resp, err := client.Get(fmt.Sprintf("https://api.quickemailverification.com/v1/verify?email=%s&apikey=%s", email, QuickEmailKey))
	if err != nil {
		return "", "", fmt.Errorf("failed to call QuickEmailVerification API: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", "", fmt.Errorf("failed to read QuickEmailVerification response: %w", err)
	}

	var apiResponse quickEmailResponse
	if err := json.Unmarshal(body, &apiResponse); err != nil {
		return "", "", fmt.Errorf("failed to parse QuickEmailVerification response: %w", err)
	}

	// Check for credit exhaustion or other errors
	if resp.StatusCode == 402 || resp.StatusCode == 429 {
		return "", "", fmt.Errorf("QuickEmailVerification credits exhausted")
	}

	if resp.StatusCode >= 400 {
		return "", "", fmt.Errorf("QuickEmailVerification API error: %s", apiResponse.Reason)
	}

	return apiResponse.Result, apiResponse.Reason, nil
}

// verifyWithMyEmailVerifier checks an email address using the MyEmailVerifier API.
func verifyWithMyEmailVerifier(email string) (string, string, error) {
	if MyEmailVerifierKey == "" {
		return ValidationUnknown, "MyEmailVerifier API key not configured", fmt.Errorf("no API key configured")
	}

	client := &http.Client{Timeout: 30 * time.Second}
	resp, err := client.Get(fmt.Sprintf("https://client.myemailverifier.com/verifier/validate_single/%s/%s", email, MyEmailVerifierKey))
	if err != nil {
		return "", "", fmt.Errorf("failed to call MyEmailVerifier API: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", "", fmt.Errorf("failed to read MyEmailVerifier response: %w", err)
	}

	var apiResponse myEmailVerifierResponse
	if err := json.Unmarshal(body, &apiResponse); err != nil {
		return "", "", fmt.Errorf("failed to parse MyEmailVerifier response: %w", err)
	}

	// Check for API errors
	if apiResponse.Status == "error" {
		if strings.Contains(strings.ToLower(apiResponse.Error.Message), "credit") ||
			strings.Contains(strings.ToLower(apiResponse.Error.Message), "balance") ||
			apiResponse.Error.Code == "insufficient_credits" {
			return "", "", fmt.Errorf("MyEmailVerifier credits exhausted: %s", apiResponse.Error.Message)
		}
		return "", "", fmt.Errorf("MyEmailVerifier API error: %s", apiResponse.Error.Message)
	}

	// Check for credit exhaustion through status code
	if resp.StatusCode == 402 || resp.StatusCode == 429 {
		return "", "", fmt.Errorf("MyEmailVerifier credits exhausted")
	}

	if resp.StatusCode >= 400 {
		return "", "", fmt.Errorf("MyEmailVerifier API error: status %d", resp.StatusCode)
	}

	// Map MyEmailVerifier status to our standard format
	result := mapMyEmailVerifierStatus(apiResponse.Result.Status)
	details := fmt.Sprintf("%s (Credits: %d available)", apiResponse.Result.Reason, apiResponse.Credits.Available)

	return result, details, nil
}

// mapMyEmailVerifierStatus maps MyEmailVerifier statuses to our standard format
func mapMyEmailVerifierStatus(status string) string {
	switch strings.ToLower(status) {
	case "valid":
		return ValidationValid
	case "invalid":
		return ValidationInvalid
	case "risky", "unknown":
		return ValidationUnknown
	default:
		return ValidationUnknown
	}
}

// isCreditsExhaustedError checks if an error indicates credit exhaustion
func isCreditsExhaustedError(errorMessage string) bool {
	lowercaseError := strings.ToLower(errorMessage)
	return strings.Contains(lowercaseError, "credit") ||
		strings.Contains(lowercaseError, "balance") ||
		strings.Contains(lowercaseError, "limit") ||
		strings.Contains(lowercaseError, "exhausted")
}

// checkDomainWithAbuseIPDB checks the email's domain IP against the AbuseIPDB blacklist.
func checkDomainWithAbuseIPDB(email string) (bool, string, error) {
	if AbuseIPDBKey == "" {
		return false, "AbuseIPDB API key not configured", nil
	}

	parts := strings.Split(email, "@")
	if len(parts) != 2 {
		return false, "Invalid email format", nil
	}
	domain := parts[1]

	// Look up the IP address for the domain
	ips, err := net.LookupIP(domain)
	if err != nil {
		return false, fmt.Sprintf("DNS lookup failed for %s", domain), err
	}
	ipAddress := ips[0].String()

	// Call AbuseIPDB API
	req, _ := http.NewRequest("GET", "https://api.abuseipdb.com/api/v2/check?ipAddress="+ipAddress, nil)
	req.Header.Add("Accept", "application/json")
	req.Header.Add("Key", AbuseIPDBKey)

	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return false, "API call failed", err
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	var apiResponse abuseIPDBResponse
	if err := json.Unmarshal(body, &apiResponse); err != nil {
		return false, "Failed to parse API response", err
	}

	score := apiResponse.Data.AbuseConfidenceScore
	details := fmt.Sprintf("Domain IP %s has abuse score of %d%%", ipAddress, score)

	isSuspicious := score > 50

	return isSuspicious, details, nil
}

// detectConflicts detects and saves conflicts for a lead
func detectConflicts(lead Lead) []Conflict {
	var conflicts []Conflict

	// Check for same domain
	domain := strings.Split(lead.Email, "@")[1]

	// NEW: Check for contacted leads with the same domain
	var contactedLeads []Lead
	db.Where("email LIKE ? AND id != ? AND status = ?", "%@"+domain, lead.ID, "contacted").Find(&contactedLeads)
	if len(contactedLeads) > 0 {
		conflict := Conflict{
			LeadID:          lead.ID,
			ConflictType:    "contacted_company_domain",
			ConflictDetails: fmt.Sprintf("Found %d contacted leads from the same domain: %s", len(contactedLeads), domain),
			CreatedAt:       time.Now(),
		}
		if err := db.Create(&conflict).Error; err != nil {
			log.Printf("Failed to save contacted_company_domain conflict for lead %d: %v", lead.ID, err)
		} else {
			conflicts = append(conflicts, conflict)
			log.Printf("Saved contacted_company_domain conflict for lead %d: %s", lead.ID, conflict.ConflictDetails)
		}
	}
	var sameDomainLeads []Lead
	db.Where("email LIKE ? AND id != ?", "%@"+domain, lead.ID).Find(&sameDomainLeads)
	if len(sameDomainLeads) > 0 {
		conflict := Conflict{
			LeadID:          lead.ID,
			ConflictType:    "same_domain",
			ConflictDetails: fmt.Sprintf("Found %d other leads with same domain: %s", len(sameDomainLeads), domain),
			CreatedAt:       time.Now(),
		}

		if err := db.Create(&conflict).Error; err != nil {
			log.Printf("Failed to save same_domain conflict for lead %d: %v", lead.ID, err)
		} else {
			conflicts = append(conflicts, conflict)
			log.Printf("Saved same_domain conflict for lead %d: %s", lead.ID, conflict.ConflictDetails)
		}
	}

	// Check for duplicate emails (should be caught by unique constraint, but let's be thorough)
	var duplicateEmails []Lead
	db.Where("email = ? AND id != ?", lead.Email, lead.ID).Find(&duplicateEmails)
	if len(duplicateEmails) > 0 {
		conflict := Conflict{
			LeadID:          lead.ID,
			ConflictType:    "duplicate_email",
			ConflictDetails: fmt.Sprintf("Duplicate email found: %d existing leads with %s", len(duplicateEmails), lead.Email),
			CreatedAt:       time.Now(),
		}

		if err := db.Create(&conflict).Error; err != nil {
			log.Printf("Failed to save duplicate_email conflict for lead %d: %v", lead.ID, err)
		} else {
			conflicts = append(conflicts, conflict)
			log.Printf("Saved duplicate_email conflict for lead %d: %s", lead.ID, conflict.ConflictDetails)
		}
	}

	// Check for similar names
	var allLeads []Lead
	db.Where("id != ?", lead.ID).Find(&allLeads)
	for _, otherLead := range allLeads {
		if calculateNameSimilarity(lead.Name, otherLead.Name) > 0.8 { // 80% threshold
			conflict := Conflict{
				LeadID:          lead.ID,
				ConflictType:    "similar_name",
				ConflictDetails: fmt.Sprintf("Similar name to '%s' (ID: %d)", otherLead.Name, otherLead.ID),
				CreatedAt:       time.Now(),
			}

			if err := db.Create(&conflict).Error; err != nil {
				log.Printf("Failed to save similar_name conflict for lead %d: %v", lead.ID, err)
			} else {
				conflicts = append(conflicts, conflict)
				log.Printf("Saved similar_name conflict for lead %d: %s", lead.ID, conflict.ConflictDetails)
			}
			break // Only report one similar name to avoid spam
		}
	}

	return conflicts
}

// processLeadComplete processes a lead completely with validation and conflict detection
func processLeadComplete(lead *Lead, job *ImportJob) (bool, error) {
	var hasConflicts bool

	// 1. Email validation (if enabled)
	if job.EnableValidation {
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
			job.EmailsValidated++

			// Track service usage in job statistics
			switch service {
			case "QuickEmailVerification":
				job.QuickEmailValidations++
			case "MyEmailVerifier":
				job.MyEmailVerifierValidations++
			}

			switch creditStatus {
			case "fallback_used":
				job.ValidationFallbacks++
			case "no_credits":
				job.ValidationCreditFailures++
			}

			log.Printf("Email validation saved for lead %d: %s (%s) via %s", lead.ID, result, details, service)
		}
	}

	// 2. Conflict detection (if enabled)
	if job.EnableConflictDetection {
		conflicts := detectConflicts(*lead)
		if len(conflicts) > 0 {
			hasConflicts = true
			job.ConflictsDetected += len(conflicts)
			log.Printf("Detected %d conflicts for lead %d", len(conflicts), lead.ID)
		}
	}

	// 3. Mark as disputed if conflicts found and auto-mark is enabled
	if hasConflicts && job.AutoMarkDisputed {
		lead.Status = "disputed"
		if err := db.Save(lead).Error; err != nil {
			log.Printf("Failed to update lead %d status to disputed: %v", lead.ID, err)
			return hasConflicts, err
		}

		// Create dispute record
		dispute := Dispute{
			LeadID:    lead.ID,
			Status:    "open",
			Notes:     fmt.Sprintf("Auto-created during import job %d due to conflicts", job.ID),
			CreatedAt: time.Now(),
		}
		if err := db.Create(&dispute).Error; err != nil {
			log.Printf("Failed to create dispute for lead %d: %v", lead.ID, err)
			return hasConflicts, err
		}

		job.LeadsMarkedDisputed++
		log.Printf("Lead %d marked as disputed and dispute record created", lead.ID)
	}

	return hasConflicts, nil
}

// calculateNameSimilarity calculates similarity between two names
func calculateNameSimilarity(name1, name2 string) float64 {
	name1 = strings.ToLower(strings.TrimSpace(name1))
	name2 = strings.ToLower(strings.TrimSpace(name2))

	distance := levenshtein.DistanceForStrings([]rune(name1), []rune(name2), levenshtein.DefaultOptions)
	maxLen := len(name1)
	if len(name2) > maxLen {
		maxLen = len(name2)
	}
	if maxLen == 0 {
		return 0.0
	}
	return 1.0 - float64(distance)/float64(maxLen)
}

// sendSMTPEmail sends an email via SMTP
func sendSMTPEmail(to, subject, body string) error {
	auth := smtp.PlainAuth("", SMTPUsername, SMTPPassword, SMTPHost)
	msg := []byte(fmt.Sprintf("To: %s\r\nSubject: %s\r\n\r\n%s\r\n", to, subject, body))
	return smtp.SendMail(SMTPHost+":"+SMTPPort, auth, SMTPUsername, []string{to}, msg)
}
