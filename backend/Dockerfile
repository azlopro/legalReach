# backend/Dockerfile

# --- Build Stage ---
# Use the official Go image as a builder
FROM golang:1.24-alpine AS builder

# Set the working directory inside the container
WORKDIR /app

# Copy go.mod and go.sum files to download dependencies first
# This leverages Docker's layer caching
COPY go.mod go.sum ./
RUN go mod download

# Copy the rest of the source code
COPY . .

# Build the application, creating a statically linked executable
RUN CGO_ENABLED=0 GOOS=linux go build -a -o /main .

# --- Final Stage ---
# Use a minimal base image for the final container
FROM alpine:latest

# Set the working directory
WORKDIR /app

# Copy the built executable from the builder stage
COPY --from=builder /main .

# Copy the SQLite database file into the container.
# This is useful if you want to ship the container with existing data.
# The volume mount in docker-compose.yml will override this during development.
COPY leads.db .

# Expose the port the application will run on (defined in main.go and .env)
EXPOSE 3000

# The command to run the application
CMD ["./main"]
