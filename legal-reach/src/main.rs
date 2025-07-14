// src/main.rs
// This file demonstrates how to use the role-based access system.

// This tells Rust to look for a `db.rs` or `db/mod.rs` file.
mod db;
mod apperr;

// Import the necessary structs and enums from our db module.
use crate::db::{Config, Database, User, UserRole};
use crate::apperr::AppError;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // --- Configuration ---
    // In a real-world application, you would load this from a configuration file
    // or environment variables, not hardcode it.
    let config = Config {
        host: "localhost".to_string(),
        user: "lead_manager_user".to_string(),
        password: "lead_manager_password".to_string(),
        dbname: "lead_management".to_string(),
    };

    // --- Database Connection ---
    // Connect to the database. If it fails, the error will be propagated up by `?`.
    let db = Database::connect(config).await?;

    // --- User Simulation ---
    // In a real application, user data would come from a login or authentication system.
    let manager_user = User {
        username: "Alice".to_string(),
        role: UserRole::Manager,
    };
    let viewer_user = User {
        username: "Bob".to_string(),
        role: UserRole::Viewer,
    };

    println!("\n--- Running simulations as Viewer: {} ---", viewer_user.username);
    
    // 1. Viewer tries to add a lead. This action should be denied.
    let viewer_add_result = db.add_lead(&viewer_user, "Viewer Added Lead", "viewer@example.com").await;
    match viewer_add_result {
        Ok(_) => println!("Viewer successfully added a lead (this should not happen)."),
        Err(AppError::PermissionDenied(msg)) => println!("Viewer failed to add lead as expected. Error: {}", msg),
        Err(e) => eprintln!("An unexpected error occurred: {}", e),
    }

    // 2. Viewer fetches all leads. This action should be permitted.
    match db.get_all_leads(&viewer_user).await {
        Ok(leads) => println!("Viewer successfully fetched {} leads.", leads.len()),
        Err(e) => println!("Viewer failed to fetch leads: {}", e),
    }

    println!("\n--- Running simulations as Manager: {} ---", manager_user.username);
    let manager_lead_name = "Managed Lead";
    let manager_lead_email = "manager@example.com";

    // 3. Manager adds a new lead. This action should be permitted.
    // We first check if the lead exists to provide a cleaner output.
    if !db.lead_exists(&manager_user, manager_lead_email).await? {
        match db.add_lead(&manager_user, manager_lead_name, manager_lead_email).await {
            Ok(_) => println!("Manager successfully added a new lead: '{}'", manager_lead_name),
            Err(e) => println!("Manager failed to add lead: {}", e),
        }
    } else {
        println!("Lead '{}' already exists.", manager_lead_name);
    }

    // 4. Manager fetches a specific lead. This action should be permitted.
    match db.get_lead(&manager_user, manager_lead_email).await {
        Ok(Some(lead)) => {
            println!(
                "Manager successfully fetched lead -> ID: {}, Name: {}, Email: {}, Joined: {}",
                lead.id, lead.name, lead.email, lead.created_at
            );
        }
        Ok(None) => println!("Manager could not find the lead."),
        Err(e) => println!("Manager failed to fetch lead: {}", e),
    }

    Ok(())
}
