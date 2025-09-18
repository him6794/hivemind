# Frontend Deployment Configuration for Cloudflare Pages

## Cloudflare Pages Settings

### Repository Configuration
- **Repository**: him6794/hivemind
- **Branch**: main (or your deployment branch)
- **Root directory**: `/frontend`

### Build Settings
- **Framework preset**: None (Static HTML)
- **Build command**: `echo "No build needed - static files"`
- **Build output directory**: `/` (root of frontend folder)

### Environment Variables
```
NODE_ENV=production
API_BASE_URL=https://hivemindapi.justin0711.com
```

### Custom Domain Setup
1. In Cloudflare Pages dashboard, go to **Custom domains**
2. Add custom domain: `hivemind.justin0711.com`
3. Configure DNS:
   - Add CNAME record: `hivemind` -> `your-pages-site.pages.dev`

### Redirects Configuration
The `_redirects` file handles SPA routing:
```
# SPA routing - redirect all routes to index.html
/login.html   /login.html   200
/register.html   /register.html   200
/dashboard.html   /dashboard.html   200
/*    /index.html   200
```

## File Structure
```
frontend/
├── index.html           # Homepage
├── login.html          # Login page
├── register.html       # Registration page
├── dashboard.html      # User dashboard
├── static/
│   ├── css/
│   │   └── main.css    # Styles
│   ├── js/
│   │   ├── api.js      # API client
│   │   ├── i18n.js     # Internationalization
│   │   └── main.js     # Main JavaScript
│   └── img/
│       ├── file.svg    # Logo
│       └── file.ico    # Favicon
└── _redirects          # Cloudflare Pages routing

```

## API Integration

The frontend automatically detects the environment and uses the appropriate API endpoint:
- **Local development**: `http://localhost:8000`
- **Production**: `https://hivemindapi.justin0711.com`

## Features Implemented

✅ **User Authentication**
- Registration with Cloudflare Turnstile
- Login with JWT tokens
- Automatic token management
- Session persistence

✅ **Dashboard**
- Balance display
- Transfer functionality
- System status monitoring
- Real-time updates

✅ **Responsive Design**
- Bootstrap 5 framework
- Mobile-friendly layout
- Modern UI components

✅ **Internationalization**
- Multi-language support (Chinese/English)
- Dynamic language switching

✅ **Security**
- CORS protection
- XSS prevention
- Secure token storage