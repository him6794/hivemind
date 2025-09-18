"""
FastAPI Backend for HiveMind
Converted from Flask app to support frontend-backend separation
API domain: hivemindapi.justin0711.com
"""

from fastapi import FastAPI, HTTPException, Depends, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from pydantic import BaseModel
from typing import Optional, Dict, Any
import os
import sys
import uvicorn
from datetime import datetime, timedelta
import uuid
import time
from collections import defaultdict
import requests
import bcrypt
import ipaddress

# Add node pool module paths (same as original Flask app)
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..', 'vpn')))
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))
from node_pool import user_service
from node_pool.config import Config

# FastAPI app initialization
app = FastAPI(
    title="HiveMind API",
    description="Backend API for HiveMind distributed computing platform",
    version="1.0.0",
    docs_url="/api/docs",
    redoc_url="/api/redoc"
)

# CORS configuration for frontend-backend separation
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "https://hivemind.justin0711.com",  # Production frontend
        "http://localhost:3000",           # Local development
        "http://localhost:8000",           # Local development
        "https://*.pages.dev"              # Cloudflare Pages preview URLs
    ],
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "DELETE", "OPTIONS"],
    allow_headers=["*"],
)

# Security
security = HTTPBearer()

# Initialize user service (same as Flask app)
try:
    user_service_obj = user_service.HivemindUserServiceServicer()
    print("✅ User service initialized successfully")
except Exception as e:
    print(f"❌ Failed to initialize user service: {e}")
    user_service_obj = None

# VPN service configuration
VPN_SERVICE_URL = f"http://{Config.VPN_SERVICE_HOST}:{Config.VPN_SERVICE_PORT}"

# Rate limiting (simplified for FastAPI)
rate_limit_data = defaultdict(list)
password_reset_rate_limit = {}

# Pydantic models for request/response
class RegisterRequest(BaseModel):
    username: str
    password: str
    cf_turnstile_response: Optional[str] = None

class LoginRequest(BaseModel):
    username: str
    password: str
    cf_turnstile_response: Optional[str] = None

class TransferRequest(BaseModel):
    receiver_username: str
    amount: float
    token: str

class UserProfileResponse(BaseModel):
    username: str
    email: Optional[str] = None
    email_verified: bool = False
    credit_score: int = 500
    balance: float = 0.0
    created_at: Optional[str] = None

class AuthResponse(BaseModel):
    access_token: str
    user: Dict[str, Any]
    message: str

# Utility functions (converted from Flask app)
def get_real_client_ip(request: Request) -> str:
    """Get real client IP supporting Cloudflare + Nginx"""
    # Try Cloudflare headers first
    cf_connecting_ip = request.headers.get('CF-Connecting-IP')
    if cf_connecting_ip and _is_valid_ip(cf_connecting_ip):
        return cf_connecting_ip
    
    # Try standard proxy headers
    x_forwarded_for = request.headers.get('X-Forwarded-For')
    if x_forwarded_for:
        ip = x_forwarded_for.split(',')[0].strip()
        if _is_valid_ip(ip):
            return ip
    
    x_real_ip = request.headers.get('X-Real-IP')
    if x_real_ip and _is_valid_ip(x_real_ip):
        return x_real_ip
    
    return request.client.host if request.client else "unknown"

def _is_valid_ip(ip_str: str) -> bool:
    """Check if string is a valid IP address"""
    try:
        ipaddress.ip_address(ip_str)
        return True
    except ValueError:
        return False

def verify_turnstile(token: str, ip_address: str) -> bool:
    """Verify Cloudflare Turnstile token"""
    if Config.is_development():
        return True
        
    if not Config.TURNSTILE_SECRET_KEY:
        return True
    
    try:
        response = requests.post(
            'https://challenges.cloudflare.com/turnstile/v0/siteverify',
            data={
                'secret': Config.TURNSTILE_SECRET_KEY,
                'response': token,
                'remoteip': ip_address
            },
            timeout=10
        )
        result = response.json()
        return result.get('success', False)
    except Exception as e:
        print(f"Turnstile verification error: {e}")
        return Config.is_development()

class MockRequest:
    """Mock gRPC request object for compatibility with existing services"""
    def __init__(self, **kwargs):
        for key, value in kwargs.items():
            setattr(self, key, value)

class MockContext:
    """Mock gRPC context for compatibility with existing services"""
    def __init__(self):
        self.code = None
        self.details = None
    
    def set_code(self, code):
        self.code = code
    
    def set_details(self, details):
        self.details = details

# Authentication dependency
def get_current_user(credentials: HTTPAuthorizationCredentials = Depends(security)):
    """Extract user from JWT token"""
    if not user_service_obj:
        raise HTTPException(status_code=500, detail="User service unavailable")
    
    try:
        token = credentials.credentials
        # Verify token using existing user service
        mock_request = MockRequest(token=token)
        response = user_service_obj.GetBalance(mock_request, MockContext())
        
        if response.success:
            return {"token": token, "balance": response.balance}
        else:
            raise HTTPException(status_code=401, detail="Invalid token")
    except Exception as e:
        raise HTTPException(status_code=401, detail="Invalid token")

# API Routes

@app.get("/api/health")
async def health_check():
    """System health check"""
    return {
        "status": "healthy",
        "timestamp": time.time(),
        "version": "1.0.0",
        "services": {
            "user_service": user_service_obj is not None,
            "vpn_service": True  # Add actual VPN service check if needed
        }
    }

@app.post("/api/register", response_model=AuthResponse)
async def register(request_data: RegisterRequest, request: Request):
    """User registration"""
    if not user_service_obj:
        raise HTTPException(status_code=500, detail="User service unavailable")
    
    # Get client IP
    client_ip = get_real_client_ip(request)
    
    # Verify Turnstile if provided
    if request_data.cf_turnstile_response:
        if not verify_turnstile(request_data.cf_turnstile_response, client_ip):
            raise HTTPException(status_code=400, detail="Human verification failed")
    
    try:
        # Use existing user service
        mock_request = MockRequest(
            username=request_data.username, 
            password=request_data.password
        )
        mock_context = MockContext()
        
        response = user_service_obj.Register(mock_request, mock_context)
        
        if response.success:
            # Auto-login after successful registration
            login_response = user_service_obj.Login(mock_request, mock_context)
            
            if login_response.success:
                return AuthResponse(
                    access_token=login_response.token,
                    user={
                        "username": request_data.username,
                        "balance": 0.0
                    },
                    message="Registration successful"
                )
            else:
                raise HTTPException(status_code=500, detail="Registration successful but login failed")
        else:
            raise HTTPException(status_code=400, detail=response.message)
            
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/api/login", response_model=AuthResponse)
async def login(request_data: LoginRequest, request: Request):
    """User login"""
    if not user_service_obj:
        raise HTTPException(status_code=500, detail="User service unavailable")
    
    # Get client IP
    client_ip = get_real_client_ip(request)
    
    # Verify Turnstile if provided
    if request_data.cf_turnstile_response:
        if not verify_turnstile(request_data.cf_turnstile_response, client_ip):
            raise HTTPException(status_code=400, detail="Human verification failed")
    
    try:
        # Use existing user service
        mock_request = MockRequest(
            username=request_data.username,
            password=request_data.password
        )
        mock_context = MockContext()
        
        response = user_service_obj.Login(mock_request, mock_context)
        
        if response.success:
            # Get user balance
            balance_request = MockRequest(token=response.token)
            balance_response = user_service_obj.GetBalance(balance_request, mock_context)
            
            balance = balance_response.balance if balance_response.success else 0.0
            
            return AuthResponse(
                access_token=response.token,
                user={
                    "username": request_data.username,
                    "balance": balance
                },
                message="Login successful"
            )
        else:
            raise HTTPException(status_code=401, detail=response.message)
            
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.get("/api/balance")
async def get_balance(current_user: dict = Depends(get_current_user)):
    """Get user balance"""
    return {"cpt_balance": current_user["balance"]}

@app.post("/api/refresh-token")
async def refresh_token(current_user: dict = Depends(get_current_user)):
    """Refresh user token"""
    # For now, return the same token (implement proper refresh logic as needed)
    return {
        "access_token": current_user["token"],
        "message": "Token refreshed successfully"
    }

@app.post("/api/transfer")
async def transfer(transfer_data: TransferRequest):
    """Transfer CPT between users"""
    if not user_service_obj:
        raise HTTPException(status_code=500, detail="User service unavailable")
    
    try:
        mock_request = MockRequest(
            token=transfer_data.token,
            amount=transfer_data.amount,
            receiver_username=transfer_data.receiver_username
        )
        mock_context = MockContext()
        
        response = user_service_obj.Transfer(mock_request, mock_context)
        
        if response.success:
            # Get updated balance
            balance_request = MockRequest(token=transfer_data.token)
            balance_response = user_service_obj.GetBalance(balance_request, mock_context)
            
            return {
                "message": "Transfer successful",
                "new_balance": balance_response.balance if balance_response.success else 0.0
            }
        else:
            raise HTTPException(status_code=400, detail=response.message)
            
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.get("/api/user/profile", response_model=UserProfileResponse)
async def get_user_profile(current_user: dict = Depends(get_current_user)):
    """Get user profile information"""
    if not user_service_obj:
        raise HTTPException(status_code=500, detail="User service unavailable")
    
    try:
        # Get profile from user service
        mock_request = MockRequest(token=current_user["token"])
        mock_context = MockContext()
        
        # This would need to be implemented in the user service
        # For now, return basic information
        return UserProfileResponse(
            username="user",  # Extract from token if available
            balance=current_user["balance"]
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

# VPN-related endpoints (simplified for now)
@app.post("/api/vpn/join")
async def join_vpn(current_user: dict = Depends(get_current_user)):
    """Join VPN network"""
    # Implement VPN joining logic
    raise HTTPException(status_code=501, detail="VPN service not yet implemented in FastAPI")

@app.get("/api/vpn/status")
async def vpn_status(current_user: dict = Depends(get_current_user)):
    """Get VPN status"""
    # Implement VPN status logic
    return {"status": "disconnected", "message": "VPN service not yet implemented"}

if __name__ == "__main__":
    print("🚀 Starting HiveMind FastAPI Backend")
    print(f"  - Environment: {Config.FLASK_ENV}")
    print(f"  - Debug mode: {Config.is_development()}")
    print(f"  - CORS origins: hivemind.justin0711.com, localhost")
    
    uvicorn.run(
        "main:app",
        host="0.0.0.0",
        port=8000,
        reload=Config.is_development(),
        log_level="info"
    )