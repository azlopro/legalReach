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

    pub async fn bulk_create_leads(&self, leads: Vec<NewLead>) -> Result<Vec<Lead>> {
        let client = self.pool.get().await?;
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

    pub async fn get_leads(&self, filters: LeadFilters, pagination: PaginationParams) -> Result<PaginatedResponse<Lead>> {
        let client = self.pool.get().await?;
        
        let page = pagination.page.unwrap_or(1);
        let per_page = pagination.per_page.unwrap_or(50).min(100); // Cap at 100
        let offset = (page - 1) * per_page;

        // Build dynamic query based on filters
        let mut query = String::from(
            "SELECT id, name, email, status, created_at, updated_at, notes, source, phone, company 
             FROM leads WHERE 1=1"
        );
        let mut count_query = String::from("SELECT COUNT(*) FROM leads WHERE 1=1");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
        let mut param_count = 0;

        // Add status filter
        if let Some(status) = &filters.status {
            param_count += 1;
            query.push_str(&format!(" AND status = ${}", param_count));
            count_query.push_str(&format!(" AND status = ${}", param_count));
            params.push(Box::new(status.to_string()));
        }

        // Add search filter (name or email)
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

        // Add date filters
        if let Some(created_after) = &filters.created_after {
            param_count += 1;
            query.push_str(&format!(" AND created_at >= ${}", param_count));
            count_query.push_str(&format!(" AND created_at >= ${}", param_count));
            params.push(Box::new(*created_after));
        }

        if let Some(created_before) = &filters.created_before {
            param_count += 1;
            query.push_str(&format!(" AND created_at <= ${}", param_count));
            count_query.push_str(&format!(" AND created_at <= ${}", param_count));
            params.push(Box::new(*created_before));
        }

        // Add ordering and pagination
        query.push_str(" ORDER BY created_at DESC");
        param_count += 1;
        query.push_str(&format!(" LIMIT ${}", param_count));
        params.push(Box::new(per_page as i64));
        
        param_count += 1;
        query.push_str(&format!(" OFFSET ${}", param_count));
        params.push(Box::new(offset as i64));

        // Execute count query
        let count_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = 
            params[..params.len()-2].iter().map(|p| p.as_ref()).collect();
        let total_count: i64 = client
            .query_one(&count_query, &count_params)
            .await?
            .get(0);

        // Execute main query
        let query_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = 
            params.iter().map(|p| p.as_ref()).collect();
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

    pub async fn update_lead(&self, id: i32, update: UpdateLead) -> Result<Option<Lead>> {
        let client = self.pool.get().await?;
        
        let mut query = String::from("UPDATE leads SET ");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
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
            params.iter().map(|p| p.as_ref()).collect();

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