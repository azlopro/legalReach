// New file: src/services/conflict_service.rs
use std::collections::HashMap;
use crate::{
    database::models::*,
    database::repository::LeadRepository,
    errors::Result,
};

pub struct ConflictDetectionService {
    settings: ConflictDetectionSettings,
}

impl ConflictDetectionService {
    pub fn new(settings: ConflictDetectionSettings) -> Self {
        Self { settings }
    }

    /// Detect conflicts for a batch of new leads
    pub async fn detect_conflicts_batch(
        &self,
        new_leads: &[NewLead],
        repository: &LeadRepository,
    ) -> Result<Vec<(usize, Vec<NewLeadConflict>)>> {
        let mut batch_conflicts = Vec::new();

        for (index, lead) in new_leads.iter().enumerate() {
            let conflicts = self.detect_conflicts_for_lead(lead, repository).await?;
            if !conflicts.is_empty() {
                batch_conflicts.push((index, conflicts));
            }
        }

        Ok(batch_conflicts)
    }

    /// Detect conflicts for a single new lead
    pub async fn detect_conflicts_for_lead(
        &self,
        new_lead: &NewLead,
        repository: &LeadRepository,
    ) -> Result<Vec<NewLeadConflict>> {
        let mut conflicts = Vec::new();

        // Check for duplicate email
        if self.settings.enable_duplicate_email_check {
            if let Some(existing_lead) = repository.get_lead_by_email(&new_lead.email).await? {
                conflicts.push(NewLeadConflict {
                    lead_id: 0, // Will be set after lead creation
                    conflict_type: ConflictType::DuplicateEmail,
                    conflict_details: format!(
                        "Email '{}' already exists for lead '{}' (ID: {})",
                        new_lead.email, existing_lead.name, existing_lead.id
                    ),
                    conflicting_lead_id: Some(existing_lead.id),
                });
            }
        }

        // Check for same domain conflicts
        if self.settings.enable_same_domain_check {
            let domain = extract_domain(&new_lead.email);
            let domain_leads = repository.get_leads_by_domain(&domain).await?;
            
            if domain_leads.len() >= self.settings.same_domain_threshold as usize {
                let domain_lead_names: Vec<String> = domain_leads
                    .iter()
                    .map(|l| format!("{} ({})", l.name, l.email))
                    .collect();
                
                conflicts.push(NewLeadConflict {
                    lead_id: 0, // Will be set after lead creation
                    conflict_type: ConflictType::SameDomain,
                    conflict_details: format!(
                        "Domain '{}' already has {} leads: {}",
                        domain,
                        domain_leads.len(),
                        domain_lead_names.join(", ")
                    ),
                    conflicting_lead_id: domain_leads.first().map(|l| l.id),
                });
            }
        }

        // Check for similar names
        if self.settings.enable_similar_name_check {
            let similar_leads = repository.find_similar_names(&new_lead.name, 10).await?;
            
            for existing_lead in similar_leads {
                let similarity = calculate_name_similarity(&new_lead.name, &existing_lead.name);
                if similarity >= self.settings.similar_name_threshold {
                    conflicts.push(NewLeadConflict {
                        lead_id: 0, // Will be set after lead creation
                        conflict_type: ConflictType::SimilarName,
                        conflict_details: format!(
                            "Name '{}' is {:.1}% similar to existing lead '{}' (ID: {})",
                            new_lead.name,
                            similarity * 100.0,
                            existing_lead.name,
                            existing_lead.id
                        ),
                        conflicting_lead_id: Some(existing_lead.id),
                    });
                }
            }
        }

        Ok(conflicts)
    }

    /// Analyze existing leads for conflicts
    pub async fn analyze_existing_leads(
        &self,
        repository: &LeadRepository,
    ) -> Result<Vec<NewLeadConflict>> {
        let mut all_conflicts = Vec::new();

        // Get all leads
        let all_leads_response = repository.get_leads(
            LeadFilters {
                status: None,
                search: None,
                source: None,
                created_after: None,
                created_before: None,
                has_conflicts: None,
                validation_result: None,
            },
            PaginationParams {
                page: Some(1),
                per_page: Some(10000),
            },
        ).await?;

        let leads = all_leads_response.data;

        // Check for same domain conflicts
        if self.settings.enable_same_domain_check {
            let domain_conflicts = self.find_domain_conflicts(&leads);
            all_conflicts.extend(domain_conflicts);
        }

        // Check for similar name conflicts
        if self.settings.enable_similar_name_check {
            let name_conflicts = self.find_name_conflicts(&leads);
            all_conflicts.extend(name_conflicts);
        }

        Ok(all_conflicts)
    }

    fn find_domain_conflicts(&self, leads: &[Lead]) -> Vec<NewLeadConflict> {
        let mut domain_map: HashMap<String, Vec<&Lead>> = HashMap::new();
        let mut conflicts = Vec::new();

        // Group leads by domain
        for lead in leads {
            let domain = extract_domain(&lead.email);
            domain_map.entry(domain).or_insert_with(Vec::new).push(lead);
        }

        // Find domains with conflicts
        for (domain, domain_leads) in domain_map {
            if domain_leads.len() >= self.settings.same_domain_threshold as usize {
                for lead in &domain_leads {
                    // Skip if already disputed
                    if lead.status == LeadStatus::Disputed {
                        continue;
                    }

                    let other_leads: Vec<String> = domain_leads
                        .iter()
                        .filter(|l| l.id != lead.id)
                        .map(|l| format!("{} ({})", l.name, l.email))
                        .collect();

                    if !other_leads.is_empty() {
                        conflicts.push(NewLeadConflict {
                            lead_id: lead.id,
                            conflict_type: ConflictType::SameDomain,
                            conflict_details: format!(
                                "Domain '{}' has {} total leads. Others: {}",
                                domain,
                                domain_leads.len(),
                                other_leads.join(", ")
                            ),
                            conflicting_lead_id: domain_leads
                                .iter()
                                .find(|l| l.id != lead.id)
                                .map(|l| l.id),
                        });
                    }
                }
            }
        }

        conflicts
    }

    fn find_name_conflicts(&self, leads: &[Lead]) -> Vec<NewLeadConflict> {
        let mut conflicts = Vec::new();

        for (i, lead1) in leads.iter().enumerate() {
            // Skip if already disputed
            if lead1.status == LeadStatus::Disputed {
                continue;
            }

            for lead2 in &leads[i + 1..] {
                let similarity = calculate_name_similarity(&lead1.name, &lead2.name);
                if similarity >= self.settings.similar_name_threshold {
                    conflicts.push(NewLeadConflict {
                        lead_id: lead1.id,
                        conflict_type: ConflictType::SimilarName,
                        conflict_details: format!(
                            "Name '{}' is {:.1}% similar to '{}' (ID: {})",
                            lead1.name,
                            similarity * 100.0,
                            lead2.name,
                            lead2.id
                        ),
                        conflicting_lead_id: Some(lead2.id),
                    });
                }
            }
        }

        conflicts
    }
}

/// Extract domain from email address
fn extract_domain(email: &str) -> String {
    email.split('@').nth(1).unwrap_or(email).to_lowercase()
}

/// Calculate similarity between two names using Levenshtein distance
fn calculate_name_similarity(name1: &str, name2: &str) -> f32 {
    let name1_lower = name1.to_lowercase();
    let name2_lower = name2.to_lowercase();
    let name1 = name1_lower.trim();
    let name2 = name2_lower.trim();

    if name1 == name2 {
        return 1.0;
    }

    let distance = levenshtein_distance(name1, name2);
    let max_len = name1.len().max(name2.len());

    if max_len == 0 {
        return 1.0;
    }

    1.0 - (distance as f32 / max_len as f32)
}

/// Calculate Levenshtein distance between two strings
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.len();
    let len2 = s2.len();

    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }

    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    // Initialize first row and column
    for i in 0..=len1 {
        matrix[i][0] = i;
    }
    for j in 0..=len2 {
        matrix[0][j] = j;
    }

    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1_chars[i - 1] == s2_chars[j - 1] { 0 } else { 1 };

            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[len1][len2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("user@example.com"), "example.com");
        assert_eq!(extract_domain("test@subdomain.example.org"), "subdomain.example.org");
        assert_eq!(extract_domain("invalid-email"), "invalid-email");
    }

    #[test]
    fn test_calculate_name_similarity() {
        assert_eq!(calculate_name_similarity("John Doe", "John Doe"), 1.0);
        assert_eq!(calculate_name_similarity("John Doe", "Jane Doe"), 0.75);
        assert_eq!(calculate_name_similarity("John", "Joan"), 0.75);
        assert!(calculate_name_similarity("John Smith", "Bob Jones") < 0.5);
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("a", ""), 1);
        assert_eq!(levenshtein_distance("", "a"), 1);
        assert_eq!(levenshtein_distance("abc", "abc"), 0);
        assert_eq!(levenshtein_distance("abc", "ab"), 1);
        assert_eq!(levenshtein_distance("abc", "def"), 3);
    }
}