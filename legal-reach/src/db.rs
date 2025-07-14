// src/db.rs
// This module contains the database interaction logic, encapsulated within a Database struct.
// It now includes a custom error type for better error handling.

use tokio_postgres::{Client, NoTls};
use serde::Deserialize;

use crate::apperr::AppError;



// --- User and Role Definitions ---
pub enum UserRole {
    Viewer,
    Manager,
}

pub struct User {
    pub username: String,
    pub role: UserRole,
}

// --- Database Configuration and Structs ---
#[derive(Deserialize)]
pub struct Config {
    pub host: String,
    pub user: String,
    pub password: String,
    pub dbname: String,
}

#[derive(Debug)]
pub struct Lead {
    pub id: i32,
    pub name: String,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct Database {
    client: Client,
}

// --- Database Implementation ---
impl Database {
    /// Connects to the database using the provided config.
    pub async fn connect(config: Config) -> Result<Self, AppError> {
        let conn_str = format!(
            "host={} user={} password={} dbname={}",
            config.host, config.user, config.password, config.dbname
        );
        let (client, connection) = tokio_postgres::connect(&conn_str, NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Connection error: {}", e);
            }
        });
        let create_table_query = "
            CREATE TABLE IF NOT EXISTS leads (
                id SERIAL PRIMARY KEY,
                name VARCHAR(255) NOT NULL,
                email VARCHAR(255) UNIQUE NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
        ";
        client.batch_execute(create_table_query).await?;
        println!("Database connection established and table initialized.");
        Ok(Self { client })
    }

    /// Adds a new lead. Only accessible to `Manager` role.
    pub async fn add_lead(&self, user: &User, name: &str, email: &str) -> Result<u64, AppError> {
        match user.role {
            UserRole::Manager => {
                let statement = "INSERT INTO leads (name, email) VALUES ($1, $2)";
                let rows_affected = self.client.execute(statement, &[&name, &email]).await?;
                Ok(rows_affected)
            },
            UserRole::Viewer => {
                // Return our custom permission error instead of a database error.
                Err(AppError::PermissionDenied(
                    "Only Managers can add leads.".to_string()
                ))
            }
        }
    }

    /// Checks if a lead exists. Accessible to all roles.
    pub async fn lead_exists(&self, _user: &User, email: &str) -> Result<bool, AppError> {
        let statement = "SELECT EXISTS(SELECT 1 FROM leads WHERE email = $1)";
        let row = self.client.query_one(statement, &[&email]).await?;
        Ok(row.get(0))
    }
    
    /// Fetches a single lead by email. Accessible to all roles.
    pub async fn get_lead(&self, _user: &User, email: &str) -> Result<Option<Lead>, AppError> {
        let stmt = "SELECT id, name, email, created_at FROM leads WHERE email = $1";
        if let Some(row) = self.client.query_opt(stmt, &[&email]).await? {
            Ok(Some(Lead {
                id: row.get("id"),
                name: row.get("name"),
                email: row.get("email"),
                created_at: row.get("created_at"),
            }))
        } else {
            Ok(None)
        }
    }

    /// Fetches all leads. Accessible to all roles.
    pub async fn get_all_leads(&self, _user: &User) -> Result<Vec<Lead>, AppError> {
        let stmt = "SELECT id, name, email, created_at FROM leads ORDER BY created_at DESC";
        let rows = self.client.query(stmt, &[]).await?;
        let leads = rows.into_iter().map(|row| Lead {
            id: row.get("id"),
            name: row.get("name"),
            email: row.get("email"),
            created_at: row.get("created_at"),
        }).collect();
        Ok(leads)
    }
}
