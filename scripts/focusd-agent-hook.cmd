@echo off
setlocal EnableExtensions DisableDelayedExpansion

REM 统一 Hook 入口：focusd-agent-hook.cmd <provider> <phase>
REM provider: codex | claudeCode
REM phase: running | completed | failed | idle
REM
REM Codex/Claude 只调用这个短 cmd 命令，避免在客户端配置中嵌入长段
REM PowerShell 表达式。Hook stdin 仍由子 PowerShell 读取，以 session_id
REM 生成稳定实例 ID，并把 turn_id 传给状态脚本。

set "PROVIDER=%~1"
set "PHASE=%~2"

if /I not "%PROVIDER%"=="codex" if /I not "%PROVIDER%"=="claudeCode" (
  exit /b 2
)

if /I not "%PHASE%"=="running" if /I not "%PHASE%"=="completed" if /I not "%PHASE%"=="failed" if /I not "%PHASE%"=="idle" (
  exit /b 2
)

set "SCRIPT_DIR=%~dp0"
set "FOCUSD_STATUS_SCRIPT=%SCRIPT_DIR%focusd-agent-status.ps1"

REM 不输出 HookResponse JSON；状态 Hook 只需要成功退出，避免不同 Codex
REM 版本对响应字段解析差异导致“hook exited with code 1”。
powershell.exe -NoProfile -ExecutionPolicy Bypass -NonInteractive -Command "[Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false; $raw = [Console]::In.ReadToEnd(); $hookInput = $null; if ($raw) { try { $raw = $raw.TrimStart([char[]]@([char]0xFEFF)); $hookInput = $raw | ConvertFrom-Json } catch { $hookInput = $null } }; $sessionId = if ($null -ne $hookInput) { [string]$hookInput.session_id } else { '' }; $instanceId = if ($sessionId) { 'session-' + ($sessionId -replace '[^a-zA-Z0-9._-]', '-') } else { '' }; $turnId = if ($null -ne $hookInput) { [string]$hookInput.turn_id } else { '' }; & $env:FOCUSD_STATUS_SCRIPT '%PROVIDER%' '%PHASE%' $instanceId $turnId"

exit /b %ERRORLEVEL%

REM === 修改记录 ===
REM 编号1：新增/修改
REM 主要修改内容：新增 Codex/Claude 共用的短命令 Hook 入口，统一解析 stdin 中的 session_id/turn_id。
REM 修改目的：避免客户端配置执行长段内联 PowerShell 时出现命令解析或 HookResponse 兼容性错误。
REM
REM 编号2：修改
REM 主要修改内容：移除会导致重定向 stdin 卡住的 -WindowStyle Hidden，改用 -NonInteractive 禁止交互提示。
REM 修改目的：确保 Codex 与 Claude Code 通过 PowerShell Unicode 启动器调用 Hook 时能及时读取 stdin 并正常退出。
REM
REM 编号3：修改
REM 主要修改内容：关闭 delayed expansion，并通过 FOCUSD_STATUS_SCRIPT 环境变量向 PowerShell 传递状态脚本路径。
REM 修改目的：兼容 AppData 路径中包含感叹号或单引号的少数 Windows 用户目录。
