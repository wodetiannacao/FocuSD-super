@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM Usage: focusd-agent-running.cmd <provider> [instanceId]
REM provider: codex | claudeCode
REM Creates a per-instance running marker so multiple windows can be tracked.

set "PROVIDER=%~1"
set "INSTANCE_ID=%~2"

if /I "%PROVIDER%"=="codex" (
  set "PROVIDER_KEY=codex"
) else if /I "%PROVIDER%"=="claudeCode" (
  set "PROVIDER_KEY=claudeCode"
) else (
  exit /b 2
)

if defined FOCUSD_AGENT_STATUS_DIR (
  set "STATUS_DIR=%FOCUSD_AGENT_STATUS_DIR%"
) else (
  if defined APPDATA (
    set "STATUS_DIR=%APPDATA%\com.focusd.island"
  ) else (
    set "STATUS_DIR=%LOCALAPPDATA%\com.focusd.island"
  )
)

if not exist "%STATUS_DIR%" mkdir "%STATUS_DIR%" >nul 2>nul

REM 统一交给 PowerShell 读取 Hook stdin 中的 session_id，避免并发 cmd 的 %RANDOM% 碰撞。
set "SCRIPT_DIR=%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "[Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false; $hookInput = [Console]::In.ReadToEnd() | ConvertFrom-Json; $sessionId = [string]$hookInput.session_id; $instanceId = if ($sessionId) { 'session-' + ($sessionId -replace '[^a-zA-Z0-9._-]', '-') } else { '%INSTANCE_ID%' }; & '%SCRIPT_DIR%focusd-agent-status.ps1' '%PROVIDER_KEY%' 'running' $instanceId ([string]$hookInput.turn_id) -HookResponse"

exit /b %ERRORLEVEL%

REM === 修改记录 ===
REM [修改编号]: 1
REM [修改日期]: 2026-07-21
REM [修改类型]: 新增功能
REM [主要内容]: 改为按 instanceId 写 per-instance running marker，并异步注册到 status 脚本
REM [修改目的]: 支持多窗口同时运行
REM [影响范围]: Codex/Claude running hook 入口

REM 编号2：修改
REM 主要修改内容：移除随机实例 ID 和直接 marker 写入，统一转发到支持 session_id 的状态脚本。
REM 修改目的：避免并发 Agent 共用同一随机 ID，并保持旧 Hook 命令兼容。

REM 编号3：新增/修改
REM 主要修改内容：旧入口按 UTF-8 解析 stdin，将 session_id 安全化后与 turn_id 一起传给状态脚本。
REM 修改目的：确保中断后继续同一对话仍使用原灯，避免遗留幽灵红灯。
