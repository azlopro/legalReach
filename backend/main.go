package main

import (
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-contrib/cors"
	"github.com/gin-gonic/gin"
)

func main() {
	// Initialize database
	if err := initDatabase(); err != nil {
		log.Fatal("Database initialization failed:", err)
	}

	// Set Gin mode
	ginMode := getEnv("GIN_MODE", "debug")
	gin.SetMode(ginMode)

	// Initialize Gin router
	r := gin.Default()

	// Configure CORS
	corsOrigins := strings.Split(getEnv("CORS_ORIGINS", "http://localhost:3000,http://localhost:8080"), ",")
	config := cors.DefaultConfig()
	config.AllowOrigins = corsOrigins
	config.AllowMethods = []string{"GET", "POST", "PUT", "DELETE", "OPTIONS"}
	config.AllowHeaders = []string{"*"}
	r.Use(cors.New(config))

	// Health check endpoint (no auth required)
	r.GET("/health", func(c *gin.Context) {
		sqlDB, err := db.DB()
		if err != nil || sqlDB.Ping() != nil {
			c.JSON(http.StatusServiceUnavailable, gin.H{"status": "unhealthy", "database": "error"})
			return
		}
		c.JSON(http.StatusOK, gin.H{
			"status":    "healthy",
			"timestamp": time.Now().UTC(),
			"database":  "connected",
			"version":   "1.0.0",
		})
	})

	// Authentication middleware
	r.Use(func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.Method == "OPTIONS" {
			c.Next()
			return
		}
		authHeader := c.GetHeader("Authorization")
		if authHeader == "" || !strings.HasPrefix(authHeader, "Bearer ") {
			c.JSON(http.StatusUnauthorized, gin.H{"error": gin.H{"message": "Authorization header required"}})
			c.Abort()
			return
		}
		token := strings.TrimPrefix(authHeader, "Bearer ")
		if token != APIKey {
			c.JSON(http.StatusUnauthorized, gin.H{"error": gin.H{"message": "Invalid API key"}})
			c.Abort()
			return
		}
		c.Next()
	})

	// API routes
	api := r.Group("/api")
	{
		// Lead routes
		api.GET("/leads", getLeads)
		api.POST("/leads/import", importLeads)
		api.GET("/leads/stats", getEnhancedStats)
		api.POST("/leads/bulk-update", bulkUpdateLeads)
		api.GET("/leads/export", exportLeads)

		// Dispute routes
		api.GET("/disputes", getDisputes)
		api.POST("/disputes/:id/resolve", resolveDispute)
		api.POST("/disputes/bulk-resolve", bulkResolveDisputes)
		api.POST("/disputes/analyze", analyzeConflicts)
		api.POST("/disputes/validate-emails", validateDisputedEmails)

		//Import routes
		api.GET("/jobs/import", listImportJobs)
		api.GET("/jobs/import/:id", getImportJobStatus)

		// Email routes
		api.POST("/email/send", sendEmails)
		api.POST("/email/send-to-zapier", sendToZapier)
	}

	port := getEnv("SERVER_PORT", "3000")
	host := getEnv("SERVER_HOST", "")

	log.Printf("Server starting on %s:%s", host, port)
	r.Run(host + ":" + port)
}
