# Run worker client (Windows)
# Usage:
#   .\run_worker.ps1 -ServerUrl http://<API_SERVER_IP>:5001
param(
  [Parameter(Mandatory=$true)][string]$ServerUrl
)

$ErrorActionPreference = 'Stop'

python -m venv .venv
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\python.exe main.py $ServerUrl
