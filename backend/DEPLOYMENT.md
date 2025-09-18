# HiveMind FastAPI Backend Deployment Configuration

## Production Deployment

### Environment Variables
Create a `.env` file in the backend directory:

```env
# FastAPI Configuration
FASTAPI_ENV=production
FASTAPI_HOST=0.0.0.0
FASTAPI_PORT=8000
FASTAPI_DEBUG=false

# CORS Settings
CORS_ORIGINS=https://hivemind.justin0711.com,https://hivemindapi.justin0711.com

# Security Keys (generate secure keys for production)
SECRET_KEY=your-secret-key-here
JWT_SECRET_KEY=your-jwt-secret-here

# Cloudflare Configuration
TURNSTILE_SECRET_KEY=your-turnstile-secret-key

# Email Service
RESEND_API_KEY=your-resend-api-key
BASE_URL=https://hivemindapi.justin0711.com

# VPN Service
VPN_SERVICE_HOST=127.0.0.1
VPN_SERVICE_PORT=8080
VPN_SERVICE_TIMEOUT=30

# Database Configuration
REDIS_HOST=localhost
REDIS_PORT=6379
```

### Docker Deployment

#### Dockerfile
```dockerfile
FROM python:3.12-slim

WORKDIR /app

# Copy requirements first for better caching
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application code
COPY . .

# Expose port
EXPOSE 8000

# Run the application
CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000", "--workers", "4"]
```

#### docker-compose.yml
```yaml
version: '3.8'

services:
  hivemind-api:
    build: .
    ports:
      - "8000:8000"
    environment:
      - FASTAPI_ENV=production
      - FASTAPI_HOST=0.0.0.0
      - FASTAPI_PORT=8000
    volumes:
      - ./.env:/app/.env
    restart: unless-stopped
    depends_on:
      - redis

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    restart: unless-stopped

volumes:
  redis_data:
```

### Systemd Service (Linux)

Create `/etc/systemd/system/hivemind-api.service`:

```ini
[Unit]
Description=HiveMind FastAPI Backend
After=network.target

[Service]
Type=simple
User=hivemind
WorkingDirectory=/opt/hivemind/backend
Environment=PATH=/opt/hivemind/venv/bin
ExecStart=/opt/hivemind/venv/bin/uvicorn main:app --host 0.0.0.0 --port 8000 --workers 4
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl enable hivemind-api
sudo systemctl start hivemind-api
```

### Nginx Configuration

Create `/etc/nginx/sites-available/hivemindapi.justin0711.com`:

```nginx
server {
    listen 80;
    server_name hivemindapi.justin0711.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name hivemindapi.justin0711.com;

    # SSL Configuration (use Let's Encrypt)
    ssl_certificate /etc/letsencrypt/live/hivemindapi.justin0711.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/hivemindapi.justin0711.com/privkey.pem;
    
    # Security Headers
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "no-referrer-when-downgrade" always;
    add_header Content-Security-Policy "default-src 'self' http: https: data: blob: 'unsafe-inline'" always;

    location / {
        proxy_pass http://127.0.0.1:8000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        
        # WebSocket support if needed
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        
        # CORS headers for preflight requests
        if ($request_method = 'OPTIONS') {
            add_header 'Access-Control-Allow-Origin' 'https://hivemind.justin0711.com' always;
            add_header 'Access-Control-Allow-Methods' 'GET, POST, OPTIONS, PUT, DELETE' always;
            add_header 'Access-Control-Allow-Headers' 'DNT,User-Agent,X-Requested-With,If-Modified-Since,Cache-Control,Content-Type,Range,Authorization' always;
            add_header 'Access-Control-Max-Age' 1728000 always;
            add_header 'Content-Type' 'text/plain; charset=utf-8' always;
            add_header 'Content-Length' 0 always;
            return 204;
        }
    }
}
```

## Frontend Deployment on Cloudflare Pages

### Build Configuration

Create `frontend/_redirects`:
```
# SPA routing - redirect all routes to index.html
/*    /index.html   200
```

Create `frontend/package.json`:
```json
{
  "name": "hivemind-frontend",
  "version": "1.0.0",
  "description": "HiveMind Frontend for Cloudflare Pages",
  "scripts": {
    "build": "echo 'No build needed - static files'",
    "dev": "python -m http.server 3000"
  }
}
```

### Cloudflare Pages Settings

1. Connect your GitHub repository
2. Set build configuration:
   - Framework preset: `None`
   - Build command: `npm run build` (optional)
   - Build output directory: `/frontend`
   - Root directory: `/frontend`

3. Environment variables (if needed):
   - `NODE_ENV`: `production`

### Custom Domain Setup

1. Go to Cloudflare Pages dashboard
2. Select your site
3. Go to "Custom domains"
4. Add `hivemind.justin0711.com`
5. Configure DNS:
   - Add CNAME record: `hivemind` -> `your-pages-site.pages.dev`

## API Testing

Test the backend endpoints:

```bash
# Health check
curl https://hivemindapi.justin0711.com/api/health

# Register (example)
curl -X POST https://hivemindapi.justin0711.com/api/register \
  -H "Content-Type: application/json" \
  -d '{"username": "testuser", "password": "testpass123"}'

# Login (example)
curl -X POST https://hivemindapi.justin0711.com/api/login \
  -H "Content-Type: application/json" \
  -d '{"username": "testuser", "password": "testpass123"}'
```

## Monitoring

### Health Check Script

Create `monitor.sh`:
```bash
#!/bin/bash
ENDPOINT="https://hivemindapi.justin0711.com/api/health"

response=$(curl -s -o /dev/null -w "%{http_code}" $ENDPOINT)

if [ $response -eq 200 ]; then
    echo "✅ API is healthy"
else
    echo "❌ API is down (HTTP $response)"
    # Add alerting logic here
fi
```

### Log Monitoring

Monitor application logs:
```bash
# For systemd service
journalctl -u hivemind-api -f

# For Docker
docker-compose logs -f hivemind-api
```