param(
  [Parameter(Mandatory = $true, Position = 0)]
  [ValidateSet("codex", "claudeCode")]
  [string]$Provider,

  [Parameter(Mandatory = $true, Position = 1)]
  [ValidateSet("idle", "running", "completed", "failed")]
  [string]$Phase,

  [Parameter(Position = 2)]
  [string]$InstanceId = "",

  [Parameter(Position = 3)]
  [string]$TaskId = "",

  [string]$StatusPath = "",

  [int]$MinimumRunningVisibleMs = 800,

  [switch]$HookResponse
)

$ErrorActionPreference = "Stop"
# 与前端最大可设置时间一致，超过24小时的 completed 实例会从持久化文件中清理。


function Get-HookInput {
  param(
    [switch]$ExpectedInput,
    [object[]]$PipelineInput = @()
  )

  try {
    # 新 Hook 已直接传入稳定实例 ID；这里仅保留旧入口的 stdin JSON 兼容读取。
    $rawInput = (($PipelineInput | ForEach-Object { [string]$_ }) -join [Environment]::NewLine)
    if (-not $rawInput -and $ExpectedInput) {
      [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false
      $rawInput = [Console]::In.ReadToEnd()
    }
    if (-not $rawInput.Trim()) {
      return $null
    }
    return $rawInput | ConvertFrom-Json
  } catch {
    # 兼容旧版或手动调用：stdin 不是合法 JSON 时继续使用随机回退逻辑。
    return $null
  }
}

function Get-StableSessionInstanceId {
  param([string]$SessionId)

  if (-not $SessionId) {
    return ""
  }

  # session_id 本身由 Codex/Claude 稳定提供；仅替换 Windows 文件名不安全字符。
  $safeId = ([string]$SessionId) -replace '[^a-zA-Z0-9._-]', '-'
  if (-not $safeId) {
    return ""
  }

  return "session-$safeId"
}

function Get-DefaultStatusPath {
  if ($env:FOCUSD_AGENT_STATUS_PATH) {
    return $env:FOCUSD_AGENT_STATUS_PATH
  }

  if ($env:APPDATA) {
    return Join-Path $env:APPDATA "com.focusd.island\agent-status.json"
  }

  return Join-Path $env:LOCALAPPDATA "com.focusd.island\agent-status.json"
}

function New-AgentInstance {
  param(
    [string]$Id,
    [string]$Provider,
    [int]$DisplayIndex = 1,
    [string]$Phase = "idle",
    [string]$TaskId = "",
    [long]$UpdatedAt = 0
  )

  $instance = [ordered]@{
    id = $Id
    provider = $Provider
    displayIndex = $DisplayIndex
    phase = $Phase
    updatedAt = $UpdatedAt
  }

  if ($TaskId) {
    $instance.taskId = $TaskId
  }

  return $instance
}

function Copy-AgentInstance {
  param([object]$Source)

  if ($null -eq $Source) {
    return $null
  }

  $id = ""
  if ($Source.PSObject.Properties.Name -contains "id") {
    $id = [string]$Source.id
  }
  if (-not $id) {
    return $null
  }

  $provider = "codex"
  if ($Source.PSObject.Properties.Name -contains "provider") {
    $candidate = [string]$Source.provider
    if (@("codex", "claudeCode") -contains $candidate) {
      $provider = $candidate
    }
  }

  $displayIndex = 1
  if ($Source.PSObject.Properties.Name -contains "displayIndex") {
    try {
      $displayIndex = [Math]::Max(1, [int]$Source.displayIndex)
    } catch {
      $displayIndex = 1
    }
  }

  $phase = "idle"
  if ($Source.PSObject.Properties.Name -contains "phase") {
    $candidatePhase = [string]$Source.phase
    if (@("idle", "running", "completed", "failed", "stale") -contains $candidatePhase) {
      $phase = $candidatePhase
    }
  }

  $updatedAt = 0
  if ($Source.PSObject.Properties.Name -contains "updatedAt") {
    $updatedAt = [long]$Source.updatedAt
  }

  $taskId = ""
  if ($Source.PSObject.Properties.Name -contains "taskId") {
    $taskId = [string]$Source.taskId
  }

  return New-AgentInstance -Id $id -Provider $provider -DisplayIndex $displayIndex -Phase $phase -TaskId $taskId -UpdatedAt $updatedAt
}

function Get-NextDisplayIndex {
  param(
    [System.Collections.IEnumerable]$Instances,
    [string]$Provider
  )

  $used = @{}
  foreach ($item in $Instances) {
    if ($null -eq $item) { continue }
    if ([string]$item.provider -ne $Provider) { continue }
    $used[[int]$item.displayIndex] = $true
  }

  $index = 1
  while ($used.ContainsKey($index)) {
    $index++
  }
  return $index
}

function Get-RunningMarkerPath {
  param(
    [string]$StatusDirectory,
    [string]$Provider,
    [string]$InstanceId
  )
  return Join-Path $StatusDirectory ("agent-running-{0}-{1}.flag" -f $Provider, $InstanceId)
}

function Get-HoldMarkerPath {
  param(
    [string]$StatusDirectory,
    [string]$Provider,
    [string]$InstanceId
  )
  return Join-Path $StatusDirectory ("agent-hold-{0}-{1}.flag" -f $Provider, $InstanceId)
}

function Get-LegacyRunningMarkerPath {
  param(
    [string]$StatusDirectory,
    [string]$Provider
  )
  if ($Provider -eq "codex") {
    return Join-Path $StatusDirectory "agent-codex-running.flag"
  }
  return Join-Path $StatusDirectory "agent-claudeCode-running.flag"
}

function Get-LegacyHoldMarkerPath {
  param(
    [string]$StatusDirectory,
    [string]$Provider
  )
  if ($Provider -eq "codex") {
    return Join-Path $StatusDirectory "agent-codex-running-hold.flag"
  }
  return Join-Path $StatusDirectory "agent-claudeCode-running-hold.flag"
}

function Find-OldestRunningInstanceId {
  param(
    [string]$StatusDirectory,
    [string]$Provider,
    [System.Collections.IEnumerable]$Instances
  )

  $pattern = "agent-running-$Provider-*.flag"
  $files = @(Get-ChildItem -LiteralPath $StatusDirectory -Filter $pattern -File -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTimeUtc)

  if ($files.Count -gt 0) {
    $name = $files[0].BaseName
    $prefix = "agent-running-$Provider-"
    if ($name.StartsWith($prefix)) {
      return $name.Substring($prefix.Length)
    }
  }

  $legacyPath = Get-LegacyRunningMarkerPath -StatusDirectory $StatusDirectory -Provider $Provider
  if (Test-Path -LiteralPath $legacyPath) {
    return "legacy"
  }

  foreach ($item in $Instances) {
    if ($null -eq $item) { continue }
    if ([string]$item.provider -ne $Provider) { continue }
    if ([string]$item.phase -eq "running") {
      return [string]$item.id
    }
  }

  return ""
}

function Update-RunningMarkers {
  param(
    [string]$Provider,
    [string]$InstanceId,
    [string]$Phase,
    [string]$StatusDirectory,
    [long]$Now,
    [int]$MinimumRunningVisibleMs
  )

  $runningPath = Get-RunningMarkerPath -StatusDirectory $StatusDirectory -Provider $Provider -InstanceId $InstanceId
  $holdPath = Get-HoldMarkerPath -StatusDirectory $StatusDirectory -Provider $Provider -InstanceId $InstanceId

  if ($Phase -eq "running") {
    [System.IO.File]::WriteAllText($runningPath, "", [System.Text.UTF8Encoding]::new($false))
    Remove-Item -LiteralPath $holdPath -Force -ErrorAction SilentlyContinue

    if ($InstanceId -eq "legacy") {
      $legacyRunning = Get-LegacyRunningMarkerPath -StatusDirectory $StatusDirectory -Provider $Provider
      $legacyHold = Get-LegacyHoldMarkerPath -StatusDirectory $StatusDirectory -Provider $Provider
      [System.IO.File]::WriteAllText($legacyRunning, "", [System.Text.UTF8Encoding]::new($false))
      Remove-Item -LiteralPath $legacyHold -Force -ErrorAction SilentlyContinue
    }
    return
  }

  $visibleUntil = 0
  if (Test-Path -LiteralPath $runningPath) {
    $markerUpdatedAt = [DateTimeOffset](Get-Item -LiteralPath $runningPath).LastWriteTimeUtc
    $elapsedMs = [Math]::Max(0, $Now - $markerUpdatedAt.ToUnixTimeMilliseconds())
    $remainingMs = [Math]::Max(0, $MinimumRunningVisibleMs - $elapsedMs)
    if ($remainingMs -gt 0) {
      $visibleUntil = $Now + $remainingMs
    }
  }

  Remove-Item -LiteralPath $runningPath -Force -ErrorAction SilentlyContinue
  if ($InstanceId -eq "legacy") {
    Remove-Item -LiteralPath (Get-LegacyRunningMarkerPath -StatusDirectory $StatusDirectory -Provider $Provider) -Force -ErrorAction SilentlyContinue
  }

  if ($visibleUntil -gt $Now) {
    [System.IO.File]::WriteAllText($holdPath, [string]$visibleUntil, [System.Text.UTF8Encoding]::new($false))
  } else {
    Remove-Item -LiteralPath $holdPath -Force -ErrorAction SilentlyContinue
  }
}

function ConvertFrom-LegacyStatus {
  param([object]$Existing, [long]$Now)

  $instances = New-Object System.Collections.Generic.List[object]

  if ($null -eq $Existing) {
    return ,$instances
  }

  if ($Existing.PSObject.Properties.Name -contains "instances" -and $null -ne $Existing.instances) {
    foreach ($item in @($Existing.instances)) {
      $copied = Copy-AgentInstance -Source $item
      if ($null -ne $copied) {
        $instances.Add($copied) | Out-Null
      }
    }
    return ,$instances
  }

  foreach ($providerName in @("codex", "claudeCode")) {
    if (-not ($Existing.PSObject.Properties.Name -contains $providerName)) {
      continue
    }
    $legacy = $Existing.$providerName
    if ($null -eq $legacy) { continue }

    $phase = "idle"
    if ($legacy.PSObject.Properties.Name -contains "phase") {
      $candidate = [string]$legacy.phase
      if (@("idle", "running", "completed", "failed", "stale") -contains $candidate) {
        $phase = $candidate
      }
    }
    if ($phase -eq "idle" -or $phase -eq "completed") {
      continue
    }

    $taskId = ""
    if ($legacy.PSObject.Properties.Name -contains "taskId") {
      $taskId = [string]$legacy.taskId
    }
    $updatedAt = $Now
    if ($legacy.PSObject.Properties.Name -contains "updatedAt") {
      $updatedAt = [long]$legacy.updatedAt
    }

    $instances.Add((New-AgentInstance -Id "legacy-$providerName" -Provider $providerName -DisplayIndex 1 -Phase $phase -TaskId $taskId -UpdatedAt $updatedAt)) | Out-Null
  }

  return ,$instances
}

if (-not $StatusPath) {
  $StatusPath = Get-DefaultStatusPath
}

$hookInput = $null
if (-not $InstanceId) {
  $hookInput = Get-HookInput -ExpectedInput:$HookResponse -PipelineInput @($input)
}
if (-not $InstanceId -and $null -ne $hookInput -and ($hookInput.PSObject.Properties.Name -contains "session_id")) {
  $InstanceId = Get-StableSessionInstanceId -SessionId ([string]$hookInput.session_id)
}
if (-not $TaskId -and $null -ne $hookInput -and ($hookInput.PSObject.Properties.Name -contains "turn_id")) {
  $TaskId = [string]$hookInput.turn_id
}

$mutex = New-Object System.Threading.Mutex($false, "FocuSD.AgentStatus")
$hasLock = $false

try {
  $hasLock = $mutex.WaitOne([TimeSpan]::FromSeconds(5))
  if (-not $hasLock) {
    throw "Timed out waiting for the FocuSD agent status file lock."
  }

  $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  $statusDirectory = Split-Path -Parent $StatusPath
  New-Item -ItemType Directory -Force -Path $statusDirectory | Out-Null

  $instances = New-Object System.Collections.Generic.List[object]
  if (Test-Path -LiteralPath $StatusPath) {
    try {
      $existing = Get-Content -LiteralPath $StatusPath -Raw | ConvertFrom-Json
      $instances = ConvertFrom-LegacyStatus -Existing $existing -Now $now
    } catch {
      $instances = New-Object System.Collections.Generic.List[object]
    }
  for ($i = $instances.Count - 1; $i -ge 0; $i--) {
    $item = $instances[$i]
    if ([string]$item.phase -ne "completed") {
      continue
    }
    $completedAt = [long]$item.updatedAt
    $completedAgeMs = [long]($now - $completedAt)
    if ($completedAt -le 0 -or $completedAgeMs -ge 86400000L) {
      $instances.RemoveAt($i)
    }
  }

  }

  if (-not $InstanceId) {
    if ($Phase -eq "running") {
      $InstanceId = [guid]::NewGuid().ToString("N").Substring(0, 12)
    } else {
      $InstanceId = Find-OldestRunningInstanceId -StatusDirectory $statusDirectory -Provider $Provider -Instances $instances
      if (-not $InstanceId) {
        $InstanceId = "legacy"
      }
    }
  }

  $InstanceId = ($InstanceId -replace '[\\/:*?"<>|\s]', '')
  if (-not $InstanceId) {
    $InstanceId = [guid]::NewGuid().ToString("N").Substring(0, 12)
  }

  Update-RunningMarkers -Provider $Provider -InstanceId $InstanceId -Phase $Phase -StatusDirectory $statusDirectory -Now $now -MinimumRunningVisibleMs $MinimumRunningVisibleMs

  $index = -1
  for ($i = 0; $i -lt $instances.Count; $i++) {
    if ([string]$instances[$i].id -eq $InstanceId -and [string]$instances[$i].provider -eq $Provider) {
      $index = $i
      break
    }
  }

  if ($Phase -eq "idle") {
    if ($index -ge 0) {
      $instances.RemoveAt($index)
    }
  } else {
    if ($index -ge 0) {
      $current = $instances[$index]
      $displayIndex = [int]$current.displayIndex
      $nextTaskId = $TaskId
      if (-not $nextTaskId -and $current.PSObject.Properties.Name -contains "taskId") {
        $nextTaskId = [string]$current.taskId
      }
      $instances[$index] = New-AgentInstance -Id $InstanceId -Provider $Provider -DisplayIndex $displayIndex -Phase $Phase -TaskId $nextTaskId -UpdatedAt $now
    } else {
      $displayIndex = Get-NextDisplayIndex -Instances $instances -Provider $Provider
      $instances.Add((New-AgentInstance -Id $InstanceId -Provider $Provider -DisplayIndex $displayIndex -Phase $Phase -TaskId $TaskId -UpdatedAt $now)) | Out-Null
    }
  }

  $state = [ordered]@{
    instances = $instances.ToArray()
    updatedAt = $now
  }

  $json = $state | ConvertTo-Json -Depth 6
  $temporaryPath = "$StatusPath.tmp"
  $utf8NoBom = New-Object System.Text.UTF8Encoding $false
  [System.IO.File]::WriteAllText($temporaryPath, $json, $utf8NoBom)
  Move-Item -LiteralPath $temporaryPath -Destination $StatusPath -Force
} finally {
  if ($hasLock) {
    $mutex.ReleaseMutex() | Out-Null
  }
  $mutex.Dispose()
}

if ($HookResponse) {
  [Console]::Out.Write('{"continue":true,"suppressOutput":true}')
}

<#
=== 修改记录 ===
[修改编号]: 1
[修改日期]: 2026-07-21
[修改类型]: 新增功能
[主要内容]:
- agent-status.json 改为 instances 多实例结构
- 支持 per-instance running/hold marker
- 兼容 legacy 单 provider 字段与 marker
[修改目的]:
- 多窗口 Agent 状态互不覆盖
[影响范围]:
- focusd-agent-status.ps1 读写协议

编号2：修改
主要修改内容：读取 Hook stdin 中的 session_id 并生成稳定、安全化实例标识；修复 PowerShell 5.1 集合兼容；completed 状态保留并在24小时后清理。
修改目的：确保多个 Codex/Claude 会话互不覆盖，支持完成灯延时消失，并避免状态文件与编号长期增长。

编号3：新增/修改
主要修改内容：同一 session_id 在不同 turn_id、中断后继续和长时间运行期间始终复用同一实例 ID。
修改目的：防止继续对话时产生第二个灯，并由 running marker 明确维持数小时任务的红灯状态。

编号4：修改
主要修改内容：新 Hook 直接传入稳定 instanceId/turn_id；completed 清理改用明确的86400000L上限，避免 PowerShell 变量作用域为空。
修改目的：确保各启动方式都不再退回随机 ID，并保证完成灯在设定保留期内不会被下一次事件提前删除。
#>

