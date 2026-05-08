@echo off
cd /d %~dp0
echo Running all executor tests...
echo.
go test -v ./pkg/executor
