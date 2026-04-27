@echo off
setlocal

cd /d "%~dp0"

echo Activating virtual environment...
call "venv\Scripts\activate.bat"

if %errorlevel% neq 0 (
    echo ERROR: Failed to activate virtual environment.
    pause
    exit /b 1
)

echo Updating hivemind_master...
pip install --upgrade hivemind_master

echo Starting HiveMind Master...
python -c "from hivemind_master import master_node; master_node.run_master_node()"

pause
endlocal
