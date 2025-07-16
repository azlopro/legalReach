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

type abuseIPDBResponse struct {
	Data struct {
		AbuseConfidenceScore int    `json:"abuseConfidenceScore"`
		Domain               string `json:"domain"`
	} `json:"data"`
}

// --- Main Validation Function ---

// validateEmail orchestrates multiple validation checks.
func validateEmail(email string) (string, string) {
	// 1. Basic format check
	emailRegex := regexp.MustCompile(`^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`)
	if !emailRegex.MatchString(email) {
		return "invalid", "Invalid email format"
	}

	// 2. QuickEmailVerification API check
	qevResult, qevDetails, err := verifyWithQuickEmail(email)
	if err != nil {
		log.Printf("QuickEmailVerification API error: %v", err)
		qevResult = "unknown"
		qevDetails = "API check failed"
	}
	// If the email is definitively invalid, stop here.
	if qevResult == "invalid" {
		return "invalid", fmt.Sprintf("Verification failed: %s", qevDetails)
	}

	// 3. AbuseIPDB domain reputation check
	isSuspicious, abuseDetails, err := checkDomainWithAbuseIPDB(email)
	if err != nil {
		log.Printf("AbuseIPDB API error: %v", err)
		abuseDetails = "API check failed"
	}

	// 4. Combine results and make a final decision
	finalDetails := fmt.Sprintf("Email Service: %s. Domain Reputation: %s.", qevDetails, abuseDetails)

	if isSuspicious {
		// If the domain is suspicious, we downgrade the status to "unknown" for manual review.
		return "unknown", finalDetails
	}

	// Otherwise, we trust the result from QuickEmailVerification.
	return qevResult, finalDetails
}

// --- API Helper Functions ---

// verifyWithQuickEmail checks an email address using the QuickEmailVerification API.
func verifyWithQuickEmail(email string) (string, string, error) {
	if QuickEmailKey == "" {
		return "unknown", "QuickEmailVerification API key not configured", nil
	}

	resp, err := http.Get(fmt.Sprintf("https://api.quickemailverification.com/v1/verify?email=%s&apikey=%s", email, QuickEmailKey))
	if err != nil {
		return "", "", fmt.Errorf("failed to call API: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	var apiResponse quickEmailResponse
	if err := json.Unmarshal(body, &apiResponse); err != nil {
		return "", "", fmt.Errorf("failed to parse API response: %w", err)
	}

	return apiResponse.Result, apiResponse.Reason, nil
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

func detectConflicts(lead Lead) []Conflict {
	var conflicts []Conflict

	// Check for same domain
	domain := strings.Split(lead.Email, "@")[1]
	var sameDomainLeads []Lead
	db.Where("email LIKE ? AND id != ?", "%@"+domain, lead.ID).Find(&sameDomainLeads)
	if len(sameDomainLeads) > 0 {
		conflicts = append(conflicts, Conflict{
			LeadID:          lead.ID,
			ConflictType:    "same_domain",
			ConflictDetails: fmt.Sprintf("Found %d other leads with same domain: %s", len(sameDomainLeads), domain),
		})
	}

	// Check for similar names
	var allLeads []Lead
	db.Where("id != ?", lead.ID).Find(&allLeads)
	for _, otherLead := range allLeads {
		if calculateNameSimilarity(lead.Name, otherLead.Name) > 0.8 { // 80% threshold
			conflicts = append(conflicts, Conflict{
				LeadID:          lead.ID,
				ConflictType:    "similar_name",
				ConflictDetails: fmt.Sprintf("Similar name to '%s'", otherLead.Name),
			})
			break // Only report one similar name
		}
	}

	return conflicts
}

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

func sendSMTPEmail(to, subject, body string) error {
	auth := smtp.PlainAuth("", SMTPUsername, SMTPPassword, SMTPHost)
	msg := []byte(fmt.Sprintf("To: %s\r\nSubject: %s\r\n\r\n%s\r\n", to, subject, body))
	return smtp.SendMail(SMTPHost+":"+SMTPPort, auth, SMTPUsername, []string{to}, msg)
}
