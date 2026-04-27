import os
from datetime import datetime
from dotenv import load_dotenv
from flask import Flask, request, jsonify
from flask_cors import CORS
from itsdangerous import URLSafeTimedSerializer, BadSignature, SignatureExpired
from sqlalchemy import create_engine, Column, Integer, String, Text, DateTime, Boolean
from sqlalchemy.orm import sessionmaker, declarative_base
import requests
import uuid  # 新增：jti 產生

# 載入環境變數
load_dotenv()

# 基本設定
SECRET_KEY = os.getenv("SECRET_KEY", "change-me")
ADMIN_PASSWORD = os.getenv("ADMIN_PASSWORD", "")
RESEND_API_KEY = os.getenv("RESEND_API_KEY", "")
FROM_EMAIL = os.getenv("FROM_EMAIL", "")
DB_PATH = os.getenv("DB_PATH", "d:/hivemind/staff/data.db")

app = Flask(__name__)
app.config["SECRET_KEY"] = SECRET_KEY
CORS(app)

# DB 設定
engine = create_engine(f"sqlite:///{DB_PATH}", connect_args={"check_same_thread": False})
SessionLocal = sessionmaker(bind=engine)
Base = declarative_base()

class Application(Base):
    __tablename__ = "applications"
    id = Column(Integer, primary_key=True)
    email = Column(String(255), nullable=False)
    github_url = Column(String(255))
    portfolio_url = Column(String(255))
    discord_id = Column(String(255))
    skills = Column(String(255))  # 以逗號分隔: e.g. "go,python,grpc"
    distributed_understanding = Column(Text)
    learning_goals = Column(Text)
    bio = Column(Text)
    status = Column(String(50), default="pending")  # pending | accepted | rejected
    reviewer_notes = Column(Text)
    result_sent = Column(Boolean, default=False)
    created_at = Column(DateTime, default=datetime.utcnow)

Base.metadata.create_all(engine)

# 新增：登出黑名單（記憶體保存，服務重啟後清空）
REVOKED_TOKENS = set()

# 管理者 Token
serializer = URLSafeTimedSerializer(SECRET_KEY, salt="admin-auth")

def create_admin_token():
    # 變更：加入 jti，搭配黑名單做登出
    jti = str(uuid.uuid4())
    return serializer.dumps({"role": "admin", "jti": jti})

def verify_admin_token(token: str):
    try:
        data = serializer.loads(token, max_age=60 * 60 * 24)  # 24h
        if data.get("role") != "admin":
            return False
        jti = data.get("jti")
        if jti and jti in REVOKED_TOKENS:
            return False
        return True
    except (BadSignature, SignatureExpired):
        return False

def admin_required(func):
    from functools import wraps
    @wraps(func)
    def wrapper(*args, **kwargs):
        auth = request.headers.get("Authorization", "")
        if not auth.startswith("Bearer "):
            return jsonify({"error": "Unauthorized"}), 401
        token = auth.split(" ", 1)[1].strip()
        if not verify_admin_token(token):
            return jsonify({"error": "Unauthorized"}), 401
        return func(*args, **kwargs)
    return wrapper

def get_bearer_token():
    # 新增：抽出取得 Bearer token
    auth = request.headers.get("Authorization", "")
    if auth.startswith("Bearer "):
        return auth.split(" ", 1)[1].strip()
    return None

# Resend 寄信
def send_result_email(to_email: str, subject: str, html_content: str):
    if not RESEND_API_KEY or not FROM_EMAIL:
        return False, "Missing RESEND_API_KEY or FROM_EMAIL"
    try:
        resp = requests.post(
            "https://api.resend.com/emails",
            headers={
                "Authorization": f"Bearer {RESEND_API_KEY}",
                "Content-Type": "application/json",
            },
            json={
                "from": FROM_EMAIL,
                "to": to_email,
                "subject": subject,
                "html": html_content,
            },
            timeout=15,
        )
        if resp.status_code >= 200 and resp.status_code < 300:
            return True, None
        return False, f"Resend error: {resp.status_code} {resp.text}"
    except Exception as e:
        return False, str(e)

# Utils
def parse_skills(skills):
    if isinstance(skills, list):
        return ",".join([s.strip().lower() for s in skills if s])
    if isinstance(skills, str):
        return ",".join([s.strip().lower() for s in skills.split(",") if s])
    return ""

def serialize_application(a: Application):
    return {
        "id": a.id,
        "email": a.email,
        "github_url": a.github_url,
        "portfolio_url": a.portfolio_url,
        "discord_id": a.discord_id,
        "skills": a.skills.split(",") if a.skills else [],
        "distributed_understanding": a.distributed_understanding,
        "learning_goals": a.learning_goals,
        "bio": a.bio,
        "status": a.status,
        "reviewer_notes": a.reviewer_notes,
        "result_sent": a.result_sent,
        "created_at": a.created_at.isoformat(),
    }

# Routes

@app.route("/api/applications", methods=["POST"])
def submit_application():
    data = request.get_json(force=True, silent=True) or {}
    email = (data.get("email") or "").strip()
    if not email:
        return jsonify({"error": "email is required"}), 400

    app_model = Application(
        email=email,
        github_url=(data.get("github_url") or "").strip(),
        portfolio_url=(data.get("portfolio_url") or "").strip(),
        discord_id=(data.get("discord_id") or "").strip(),
        skills=parse_skills(data.get("skills")),
        distributed_understanding=(data.get("distributed_understanding") or "").strip(),
        learning_goals=(data.get("learning_goals") or "").strip(),
        bio=(data.get("bio") or "").strip(),
        status="pending",
    )
    db = SessionLocal()
    try:
        db.add(app_model)
        db.commit()
        db.refresh(app_model)
        return jsonify({"ok": True, "id": app_model.id}), 201
    finally:
        db.close()

@app.route("/api/admin/login", methods=["POST"])
def admin_login():
    data = request.get_json(force=True, silent=True) or {}
    pw = data.get("password") or ""
    if not ADMIN_PASSWORD:
        return jsonify({"error": "ADMIN_PASSWORD not set"}), 500
    if pw != ADMIN_PASSWORD:
        return jsonify({"error": "Invalid password"}), 401
    token = create_admin_token()
    return jsonify({"token": token})

@app.route("/api/admin/me", methods=["GET"])
@admin_required
def admin_me():
    # 新增：供前端檢查是否已登入
    return jsonify({"ok": True, "role": "admin"})

@app.route("/api/admin/logout", methods=["POST"])
def admin_logout():
    # 新增：登出並讓 token 失效
    token = get_bearer_token()
    if not token:
        return jsonify({"error": "Unauthorized"}), 401
    try:
        data = serializer.loads(token, max_age=60 * 60 * 24)
        if data.get("role") != "admin":
            return jsonify({"error": "Unauthorized"}), 401
        jti = data.get("jti")
        if jti:
            REVOKED_TOKENS.add(jti)
        return jsonify({"ok": True})
    except (BadSignature, SignatureExpired):
        return jsonify({"error": "Unauthorized"}), 401

@app.route("/api/admin/applications", methods=["GET"])
@admin_required
def list_applications():
    db = SessionLocal()
    try:
        rows = db.query(Application).order_by(Application.created_at.desc()).all()
        return jsonify([serialize_application(r) for r in rows])
    finally:
        db.close()

@app.route("/api/admin/applications/<int:app_id>", methods=["GET"])
@admin_required
def get_application(app_id: int):
    db = SessionLocal()
    try:
        a = db.get(Application, app_id)
        if not a:
            return jsonify({"error": "Not found"}), 404
        return jsonify(serialize_application(a))
    finally:
        db.close()

@app.route("/api/admin/applications/<int:app_id>/review", methods=["PUT"])
@admin_required
def review_application(app_id: int):
    data = request.get_json(force=True, silent=True) or {}
    status = (data.get("status") or "").strip().lower()
    reviewer_notes = (data.get("reviewer_notes") or "").strip()
    send_email_flag = bool(data.get("send_email"))

    if status not in ("pending", "accepted", "rejected"):
        return jsonify({"error": "Invalid status"}), 400

    db = SessionLocal()
    try:
        a = db.get(Application, app_id)
        if not a:
            return jsonify({"error": "Not found"}), 404

        a.status = status
        a.reviewer_notes = reviewer_notes

        email_sent = False
        email_error = None

        if send_email_flag:
            subject = "招募結果通知"
            if status == "accepted":
                body = f"""
                <p>您好，感謝您申請參與！</p>
                <p>恭喜，您的申請已通過。我們非常期待與您合作。</p>
                {f'<p>備註：{reviewer_notes}</p>' if reviewer_notes else ''}
                <p>— Hivemind 團隊</p>
                """
            elif status == "rejected":
                body = f"""
                <p>您好，感謝您申請參與。</p>
                <p>此次未能通過，非常遺憾。我們歡迎您日後再嘗試。</p>
                {f'<p>原因／建議：{reviewer_notes}</p>' if reviewer_notes else ''}
                <p>— Hivemind 團隊</p>
                """
            else:
                body = f"""
                <p>您好，您的申請目前仍在審核中。</p>
                {f'<p>備註：{reviewer_notes}</p>' if reviewer_notes else ''}
                <p>— Hivemind 團隊</p>
                """
            ok, err = send_result_email(a.email, subject, body)
            email_sent = ok
            email_error = err
            if ok:
                a.result_sent = True

        db.commit()
        resp = serialize_application(a)
        resp["email_sent"] = email_sent
        if email_error:
            resp["email_error"] = email_error
        return jsonify(resp)
    finally:
        db.close()

if __name__ == "__main__":
    port = int(os.getenv("PORT", "5000"))
    app.run(host="0.0.0.0", port=port, debug=False)
