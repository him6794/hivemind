# Run API server (Windows)
$ErrorActionPreference = 'Stop'

python -m venv .venv
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\python.exe api.py
