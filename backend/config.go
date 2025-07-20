package main

import (
	"fmt"
	"log"
	"os"
	"time"

	"gorm.io/driver/postgres"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
)

// Global database instance
var db *gorm.DB

// Email and API Key configuration
var (
	SMTPHost               = getEnv("SMTP_HOST", "smtp.gmail.com")
	SMTPPort               = getEnv("SMTP_PORT", "587")
	SMTPUsername           = getEnv("SMTP_USERNAME", "your-email@gmail.com")
	SMTPPassword           = getEnv("SMTP_PASSWORD", "your-app-password")
	ZapierEmail            = getEnv("ZAPIER_EMAIL", "odf86lbl@robot.zapier.com")
	APIKey                 = getEnv("API_KEY", "your-secure-api-key-here")
	QuickEmailKey          = getEnv("QUICKEMAILVERIFICATION_API_KEY", "")
	MyEmailVerifierKey     = getEnv("MYEMAILVERIFIER_API_KEY", "") // NEW: MyEmailVerifier API key
	AbuseIPDBKey           = getEnv("ABUSEIPDB_API_KEY", "")
	GoogleSheetID          = getEnv("GOOGLE_SHEET_ID", "your-spreadsheet-id-here")
	GoogleSheetCredentials = getEnv("GOOGLE_SHEET_CREDENTIALS", "your-credentials.json")
)

// getEnv retrieves an environment variable or returns a default value.
func getEnv(key, defaultValue string) string {
	if value, exists := os.LookupEnv(key); exists {
		return value
	}
	return defaultValue
}

// initDatabase connects to the database and performs auto-migration.
func initDatabase() error {
	var err error
	dbType := getEnv("DB_TYPE", "sqlite")

	switch dbType {
	case "postgres":
		dsn := fmt.Sprintf("host=%s user=%s password=%s dbname=%s port=%s sslmode=%s TimeZone=UTC",
			getEnv("DB_HOST", "localhost"),
			getEnv("DB_USER", "user"),
			getEnv("DB_PASSWORD", "password"),
			getEnv("DB_NAME", "leads_db"),
			getEnv("DB_PORT", "5432"),
			getEnv("DB_SSLMODE", "disable"))
		db, err = gorm.Open(postgres.Open(dsn), &gorm.Config{})
		if err != nil {
			return fmt.Errorf("failed to connect to PostgreSQL: %v", err)
		}

		sqlDB, _ := db.DB()
		sqlDB.SetMaxIdleConns(10)
		sqlDB.SetMaxOpenConns(100)
		sqlDB.SetConnMaxLifetime(time.Hour)

	case "sqlite":
		db, err = gorm.Open(sqlite.Open(getEnv("DB_PATH", "leads.db")), &gorm.Config{})
		if err != nil {
			return fmt.Errorf("failed to connect to SQLite: %v", err)
		}

	default:
		return fmt.Errorf("unsupported database type: %s", dbType)
	}

	if err := db.AutoMigrate(&Lead{}, &Conflict{}, &Validation{}, &Dispute{}, &ImportJob{}); err != nil {
		return fmt.Errorf("failed to migrate database: %v", err)
	}

	log.Printf("Database initialized successfully (%s)", dbType)
	return nil
}
