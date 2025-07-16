# frontend/Dockerfile

# Use the official Nginx image on Alpine for a small footprint
FROM nginx:stable-alpine

# Copy the static HTML file into the default Nginx public directory
COPY ./index.html /usr/share/nginx/html/index.html

# Expose port 80 for the Nginx server
EXPOSE 80

# The default Nginx command will start the server automatically
