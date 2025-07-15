// Enhanced src/database/repository.rs with conflict and validation support
use deadpool_postgres::Pool;
use tokio_postgres::Row;
use crate::{
    database::models::*,
    errors::{AppError, Result},
};

#[derive(Clone)]
pub struct LeadRepository {
    pool: Pool,
}

impl LeadRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    // Existing methods remain the same...
    pub async fn create_lead(&self, new_lead: NewLead) -> Result<Lead> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_one(
                "INSERT INTO leads (name, email, notes, source, phone, company) 
                 VALUES ($1, $2, $3, $4, $5, $6) 
                 RETURNING id, name, email, status, created_at, updated_at, notes, source, phone, company",
                &[
                    &new_lead.name,
                    &new_lead.email,
                    &new_lead.notes,
                    &new_lead.source,
                    &new_lead.phone,
                    &new_lead.company,
                ],
            )
            .await
            .map_err(|e| {
                if e.to_string().contains("duplicate key") {
                    AppError::conflict("Lead with this email already exists")
                } else {
                    AppError::from(e)
                }
            })?;

        Ok(row_to_lead(row))
    }

    pub async fn get_lead_by_email(&self, email: &str) -> Result<Option<Lead>> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_opt(
                "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
                 FROM leads WHERE email = $1",
                &[&email],
            )
            .await?;

        Ok(row.map(row_to_lead))
    }

    // NEW METHODS FOR CONFLICT DETECTION

    /// Get leads by domain
    pub async fn get_leads_by_domain(&self, domain: &str) -> Result<Vec<Lead>> {
        let client = self.pool.get().await?;
        
        let rows = client
            .query(
                "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
                 FROM leads WHERE email ILIKE $1",
                &[&format!("%@{}", domain)],
            )
            .await?;

        Ok(rows.into_iter().map(row_to_lead).collect())
    }

    /// Find leads with similar names
    pub async fn find_similar_names(&self, name: &str, limit: i64) -> Result<Vec<Lead>> {
        let client = self.pool.get().await?;
        
        // Use PostgreSQL's similarity function if available, otherwise use ILIKE
        let rows = client
            .query(
                "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
                 FROM leads 
                 WHERE name ILIKE $1 AND name != $2
                 ORDER BY name
                 LIMIT $3",
                &[&format!("%{}%", name), &name, &limit],
            )
            .await?;

        Ok(rows.into_iter().map(row_to_lead).collect())
    }

    // CONFLICT MANAGEMENT METHODS

    /// Create a conflict record
    pub async fn create_conflict(&self, conflict: NewLeadConflict) -> Result<LeadConflict> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_one(
                "INSERT INTO lead_conflicts (lead_id, conflict_type, conflict_details, conflicting_lead_id) 
                 VALUES ($1, $2, $3, $4) 
                 RETURNING id, lead_id, conflict_type, conflict_details, conflicting_lead_id, created_at, resolved_at, resolution_action",
                &[
                    &conflict.lead_id,
                    &conflict.conflict_type.to_string(),
                    &conflict.conflict_details,
                    &conflict.conflicting_lead_id,
                ],
            )
            .await?;

        Ok(row_to_conflict(row))
    }

    /// Get conflicts for a specific lead
    pub async fn get_conflicts_for_lead(&self, lead_id: i32) -> Result<Vec<LeadConflict>> {
        let client = self.pool.get().await?;
        
        let rows = client
            .query(
                "SELECT id, lead_id, conflict_type, conflict_details, conflicting_lead_id, created_at, resolved_at, resolution_action 
                 FROM lead_conflicts WHERE lead_id = $1 AND resolved_at IS NULL
                 ORDER BY created_at DESC",
                &[&lead_id],
            )
            .await?;

        Ok(rows.into_iter().map(row_to_conflict).collect())
    }

    /// Get all disputed leads with their conflicts
    pub async fn get_disputed_leads(&self, pagination: PaginationParams) -> Result<PaginatedResponse<LeadWithDetails>> {
        let client = self.pool.get().await?;
        
        let page = pagination.page.unwrap_or(1);
        let per_page = pagination.per_page.unwrap_or(50).min(100);
        let offset = (page - 1) * per_page;

        // Get disputed leads
        let lead_rows = client
            .query(
                "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
                 FROM leads 
                 WHERE status = 'disputed'
                 ORDER BY created_at DESC
                 LIMIT $1 OFFSET $2",
                &[&(per_page as i64), &(offset as i64)],
            )
            .await?;

        let mut leads_with_details = Vec::new();

        for lead_row in lead_rows {
            let lead = row_to_lead(lead_row);
            let conflicts = self.get_conflicts_for_lead(lead.id).await?;
            let validations = self.get_validations_for_lead(lead.id).await?;

            leads_with_details.push(LeadWithDetails {
                lead,
                conflicts,
                validations,
            });
        }

        // Get total count
        let total_count: i64 = client
            .query_one("SELECT COUNT(*) FROM leads WHERE status = 'disputed'", &[])
            .await?
            .get(0);

        let total_pages = ((total_count as f64) / (per_page as f64)).ceil() as u32;

        Ok(PaginatedResponse {
            data: leads_with_details,
            pagination: PaginationInfo {
                current_page: page,
                per_page,
                total_items: total_count,
                total_pages,
                has_next: page < total_pages,
                has_prev: page > 1,
            },
        })
    }

    /// Resolve a dispute
    pub async fn resolve_dispute(
        &self,
        lead_id: i32,
        resolution: DisputeResolution,
        notes: Option<String>,
    ) -> Result<Lead> {
        let mut client = self.pool.get().await?;
        
        // Start transaction
        let transaction = client.transaction().await?;

        // Mark conflicts as resolved
        transaction
            .execute(
                "UPDATE lead_conflicts 
                 SET resolved_at = NOW(), resolution_action = $1 
                 WHERE lead_id = $2 AND resolved_at IS NULL",
                &[&resolution.to_string(), &lead_id],
            )
            .await?;

        // Update lead status based on resolution
        let new_status = match resolution {
            DisputeResolution::Accept => LeadStatus::New,
            DisputeResolution::MarkContacted => LeadStatus::Contacted,
            DisputeResolution::Discard => LeadStatus::Lost,
        };

        // Update lead status and notes
        let update_notes = notes.unwrap_or_else(|| {
            format!("Dispute resolved: {}", resolution.to_string())
        });

        let row = transaction
            .query_one(
                "UPDATE leads 
                 SET status = $1, notes = COALESCE(notes || E'\n\n', '') || $2
                 WHERE id = $3
                 RETURNING id, name, email, status, created_at, updated_at, notes, source, phone, company",
                &[&new_status.to_string(), &update_notes, &lead_id],
            )
            .await?;

        transaction.commit().await?;

        Ok(row_to_lead(row))
    }

    // VALIDATION MANAGEMENT METHODS

    /// Create a validation record
    pub async fn create_validation(&self, validation: NewLeadValidation) -> Result<LeadValidation> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_one(
                "INSERT INTO lead_validations (lead_id, validation_type, result, details) 
                 VALUES ($1, $2, $3, $4) 
                 RETURNING id, lead_id, validation_type, result, details, validated_at",
                &[
                    &validation.lead_id,
                    &validation.validation_type.to_string(),
                    &validation.result.to_string(),
                    &validation.details,
                ],
            )
            .await?;

        Ok(row_to_validation(row))
    }

    /// Get validations for a specific lead
    pub async fn get_validations_for_lead(&self, lead_id: i32) -> Result<Vec<LeadValidation>> {
        let client = self.pool.get().await?;
        
        let rows = client
            .query(
                "SELECT id, lead_id, validation_type, result, details, validated_at 
                 FROM lead_validations WHERE lead_id = $1
                 ORDER BY validated_at DESC",
                &[&lead_id],
            )
            .await?;

        Ok(rows.into_iter().map(row_to_validation).collect())
    }

    /// Get leads with specific validation results
    pub async fn get_leads_by_validation(
        &self,
        validation_type: ValidationType,
        result: ValidationResult,
        pagination: PaginationParams,
    ) -> Result<PaginatedResponse<Lead>> {
        let client = self.pool.get().await?;
        
        let page = pagination.page.unwrap_or(1);
        let per_page = pagination.per_page.unwrap_or(50).min(100);
        let offset = (page - 1) * per_page;

        let rows = client
            .query(
                "SELECT DISTINCT l.id, l.name, l.email, l.status, l.created_at, l.updated_at, l.notes, l.source, l.phone, l.company 
                 FROM leads l
                 JOIN lead_validations v ON l.id = v.lead_id
                 WHERE v.validation_type = $1 AND v.result = $2
                 ORDER BY l.created_at DESC
                 LIMIT $3 OFFSET $4",
                &[
                    &validation_type.to_string(),
                    &result.to_string(),
                    &(per_page as i64),
                    &(offset as i64),
                ],
            )
            .await?;

        let leads: Vec<Lead> = rows.into_iter().map(row_to_lead).collect();

        // Get total count
        let total_count: i64 = client
            .query_one(
                "SELECT COUNT(DISTINCT l.id) 
                 FROM leads l
                 JOIN lead_validations v ON l.id = v.lead_id
                 WHERE v.validation_type = $1 AND v.result = $2",
                &[&validation_type.to_string(), &result.to_string()],
            )
            .await?
            .get(0);

        let total_pages = ((total_count as f64) / (per_page as f64)).ceil() as u32;

        Ok(PaginatedResponse {
            data: leads,
            pagination: PaginationInfo {
                current_page: page,
                per_page,
                total_items: total_count,
                total_pages,
                has_next: page < total_pages,
                has_prev: page > 1,
            },
        })
    }

    // STATISTICS METHODS

    /// Get dispute statistics
    pub async fn get_dispute_stats(&self) -> Result<DisputeStats> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_one(
                "SELECT 
                    COUNT(*) FILTER (WHERE status = 'disputed') as total_disputed,
                    COUNT(*) FILTER (WHERE status = 'disputed' AND id IN (
                        SELECT lead_id FROM lead_conflicts WHERE conflict_type = 'same_domain' AND resolved_at IS NULL
                    )) as same_domain_conflicts,
                    COUNT(*) FILTER (WHERE status = 'disputed' AND id IN (
                        SELECT lead_id FROM lead_conflicts WHERE conflict_type = 'duplicate_email' AND resolved_at IS NULL
                    )) as duplicate_email_conflicts,
                    COUNT(*) FILTER (WHERE status = 'disputed' AND id IN (
                        SELECT lead_id FROM lead_conflicts WHERE conflict_type = 'similar_name' AND resolved_at IS NULL
                    )) as similar_name_conflicts
                 FROM leads",
                &[],
            )
            .await?;

        Ok(DisputeStats {
            total_disputed: row.get("total_disputed"),
            same_domain_conflicts: row.get("same_domain_conflicts"),
            duplicate_email_conflicts: row.get("duplicate_email_conflicts"),
            similar_name_conflicts: row.get("similar_name_conflicts"),
        })
    }

    /// Get validation statistics
    pub async fn get_validation_stats(&self) -> Result<ValidationStats> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_one(
                "SELECT 
                    COUNT(*) as total_validations,
                    COUNT(*) FILTER (WHERE result = 'valid') as valid_emails,
                    COUNT(*) FILTER (WHERE result = 'invalid') as invalid_emails,
                    COUNT(*) FILTER (WHERE result = 'pending') as pending_validations
                 FROM lead_validations 
                 WHERE validation_type = 'email_active'",
                &[],
            )
            .await?;

        Ok(ValidationStats {
            total_validations: row.get("total_validations"),
            valid_emails: row.get("valid_emails"),
            invalid_emails: row.get("invalid_emails"),
            pending_validations: row.get("pending_validations"),
        })
    }

    // Keep all existing methods (simplified for brevity)
    pub async fn bulk_create_leads(&self, leads: Vec<NewLead>) -> Result<Vec<Lead>> {
        let mut created_leads = Vec::new();
        
        for lead in leads {
            match self.create_lead(lead).await {
                Ok(created_lead) => created_leads.push(created_lead),
                Err(AppError::Conflict { .. }) => {
                    // Skip duplicates, continue with next lead
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        
        Ok(created_leads)
    }

    pub async fn get_lead_by_id(&self, id: i32) -> Result<Option<Lead>> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_opt(
                "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
                 FROM leads WHERE id = $1",
                &[&id],
            )
            .await?;

        Ok(row.map(row_to_lead))
    }

    pub async fn get_leads(&self, filters: LeadFilters, pagination: PaginationParams) -> Result<PaginatedResponse<Lead>> {
        let client = self.pool.get().await?;
        
        let page = pagination.page.unwrap_or(1);
        let per_page = pagination.per_page.unwrap_or(50).min(100);
        let offset = (page - 1) * per_page;

        let mut query = String::from(
            "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
            FROM leads WHERE 1=1"
        );
        let mut count_query = String::from("SELECT COUNT(*) FROM leads WHERE 1=1");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        let mut param_count = 0;

        // Add status filter
        if let Some(status) = &filters.status {
            param_count += 1;
            query.push_str(&format!(" AND status = ${}", param_count));
            count_query.push_str(&format!(" AND status = ${}", param_count));
            params.push(Box::new(status.to_string()));
        }

        // Add search filter
        if let Some(search) = &filters.search {
            if !search.trim().is_empty() {
                param_count += 1;
                query.push_str(&format!(" AND (name ILIKE ${} OR email ILIKE ${})", param_count, param_count));
                count_query.push_str(&format!(" AND (name ILIKE ${} OR email ILIKE ${})", param_count, param_count));
                params.push(Box::new(format!("%{}%", search)));
            }
        }

        // Add source filter
        if let Some(source) = &filters.source {
            param_count += 1;
            query.push_str(&format!(" AND source = ${}", param_count));
            count_query.push_str(&format!(" AND source = ${}", param_count));
            params.push(Box::new(source.clone()));
        }

        // Add conflict filter
        if let Some(has_conflicts) = filters.has_conflicts {
            if has_conflicts {
                query.push_str(" AND id IN (SELECT lead_id FROM lead_conflicts WHERE resolved_at IS NULL)");
                count_query.push_str(" AND id IN (SELECT lead_id FROM lead_conflicts WHERE resolved_at IS NULL)");
            } else {
                query.push_str(" AND id NOT IN (SELECT lead_id FROM lead_conflicts WHERE resolved_at IS NULL)");
                count_query.push_str(" AND id NOT IN (SELECT lead_id FROM lead_conflicts WHERE resolved_at IS NULL)");
            }
        }

        // Add ordering and pagination
        query.push_str(" ORDER BY created_at DESC");
        param_count += 1;
        query.push_str(&format!(" LIMIT ${}", param_count));
        params.push(Box::new(per_page as i64));
        
        param_count += 1;
        query.push_str(&format!(" OFFSET ${}", param_count));
        params.push(Box::new(offset as i64));

        // Execute queries (simplified error handling for brevity)
        let count_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = 
            params[..params.len()-2].iter().map(|p| &**p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();
        let total_count: i64 = client.query_one(&count_query, &count_params).await?.get(0);

        let query_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = 
            params.iter().map(|p| &**p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();
        let rows = client.query(&query, &query_params).await?;
        
        let leads: Vec<Lead> = rows.into_iter().map(row_to_lead).collect();
        let total_pages = ((total_count as f64) / (per_page as f64)).ceil() as u32;
        
        Ok(PaginatedResponse {
            data: leads,
            pagination: PaginationInfo {
                current_page: page,
                per_page,
                total_items: total_count,
                total_pages,
                has_next: page < total_pages,
                has_prev: page > 1,
            },
        })
    }

    // Keep other existing methods...
    pub async fn update_lead(&self, id: i32, update: UpdateLead) -> Result<Option<Lead>> {
        let client = self.pool.get().await?;
        
        let mut query = String::from("UPDATE leads SET ");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        let mut param_count = 0;
        let mut updates = Vec::new();

        if let Some(name) = update.name {
            param_count += 1;
            updates.push(format!("name = ${}", param_count));
            params.push(Box::new(name));
        }

        if let Some(email) = update.email {
            param_count += 1;
            updates.push(format!("email = ${}", param_count));
            params.push(Box::new(email));
        }

        if let Some(status) = update.status {
            param_count += 1;
            updates.push(format!("status = ${}", param_count));
            params.push(Box::new(status.to_string()));
        }

        if let Some(notes) = update.notes {
            param_count += 1;
            updates.push(format!("notes = ${}", param_count));
            params.push(Box::new(notes));
        }

        if let Some(phone) = update.phone {
            param_count += 1;
            updates.push(format!("phone = ${}", param_count));
            params.push(Box::new(phone));
        }

        if let Some(company) = update.company {
            param_count += 1;
            updates.push(format!("company = ${}", param_count));
            params.push(Box::new(company));
        }

        if updates.is_empty() {
            return self.get_lead_by_id(id).await;
        }

        query.push_str(&updates.join(", "));
        param_count += 1;
        query.push_str(&format!(" WHERE id = ${} RETURNING id, name, email, status, created_at, updated_at, notes, source, phone, company", param_count));
        params.push(Box::new(id));

        let query_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = 
            params.iter().map(|p| &**p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();

        let row = client
            .query_opt(&query, &query_params)
            .await?;

        Ok(row.map(row_to_lead))
    }

    pub async fn update_leads_status(&self, ids: Vec<i32>, status: LeadStatus) -> Result<u64> {
        let client = self.pool.get().await?;
        
        let rows_affected = client
            .execute(
                "UPDATE leads SET status = $1 WHERE id = ANY($2)",
                &[&status.to_string(), &ids],
            )
            .await?;

        Ok(rows_affected)
    }

    pub async fn delete_lead(&self, id: i32) -> Result<bool> {
        let client = self.pool.get().await?;
        
        let rows_affected = client
            .execute("DELETE FROM leads WHERE id = $1", &[&id])
            .await?;

        Ok(rows_affected > 0)
    }

    pub async fn get_leads_by_ids(&self, ids: Vec<i32>) -> Result<Vec<Lead>> {
        let client = self.pool.get().await?;
        
        let rows = client
            .query(
                "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
                 FROM leads WHERE id = ANY($1) ORDER BY created_at DESC",
                &[&ids],
            )
            .await?;

        Ok(rows.into_iter().map(row_to_lead).collect())
    }

    // Email log methods
    pub async fn create_email_log(&self, email_log: NewEmailLog) -> Result<EmailLog> {
        let client = self.pool.get().await?;
        
        let row = client
            .query_one(
                "INSERT INTO email_logs (lead_id, subject, body) 
                 VALUES ($1, $2, $3) 
                 RETURNING id, lead_id, subject, body, sent_at, status",
                &[&email_log.lead_id, &email_log.subject, &email_log.body],
            )
            .await?;

        Ok(EmailLog {
            id: row.get("id"),
            lead_id: row.get("lead_id"),
            subject: row.get("subject"),
            body: row.get("body"),
            sent_at: row.get("sent_at"),
            status: row.get("status"),
        })
    }

    pub async fn get_email_logs_for_lead(&self, lead_id: i32) -> Result<Vec<EmailLog>> {
        let client = self.pool.get().await?;
        
        let rows = client
            .query(
                "SELECT id, lead_id, subject, body, sent_at, status 
                 FROM email_logs WHERE lead_id = $1 ORDER BY sent_at DESC",
                &[&lead_id],
            )
            .await?;

        Ok(rows.into_iter().map(|row| EmailLog {
            id: row.get("id"),
            lead_id: row.get("lead_id"),
            subject: row.get("subject"),
            body: row.get("body"),
            sent_at: row.get("sent_at"),
            status: row.get("status"),
        }).collect())
    }
}

// Helper functions
fn row_to_lead(row: Row) -> Lead {
    Lead {
        id: row.get("id"),
        name: row.get("name"),
        email: row.get("email"),
        status: LeadStatus::from(row.get::<_, String>("status")),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        notes: row.get("notes"),
        source: row.get("source"),
        phone: row.get("phone"),
        company: row.get("company"),
    }
}

fn row_to_conflict(row: Row) -> LeadConflict {
    LeadConflict {
        id: row.get("id"),
        lead_id: row.get("lead_id"),
        conflict_type: ConflictType::from(row.get::<_, String>("conflict_type")),
        conflict_details: row.get("conflict_details"),
        conflicting_lead_id: row.get("conflicting_lead_id"),
        created_at: row.get("created_at"),
        resolved_at: row.get("resolved_at"),
        resolution_action: row.get("resolution_action"),
    }
}

fn row_to_validation(row: Row) -> LeadValidation {
    LeadValidation {
        id: row.get("id"),
        lead_id: row.get("lead_id"),
        validation_type: ValidationType::from(row.get::<_, String>("validation_type")),
        result: ValidationResult::from(row.get::<_, String>("result")),
        details: row.get("details"),
        validated_at: row.get("validated_at"),
    }
}