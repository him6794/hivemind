@echo off
cd /d %~dp0
go test -v ./pkg/executor -run TestExecuteTask_SimpleScript
