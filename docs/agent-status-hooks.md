# AI Agent Status Hooks

FocuSD can mirror Codex and Claude Code task activity on the collapsed island.

## Multi-instance model

Each running agent window/session is tracked as a separate instance:

- marker file: `agent-running-{provider}-{instanceId}.flag`
- hold file: `agent-hold-{provider}-{instanceId}.flag`
- status file: `agent-status.json` with an `instances` array

Display names look like:

- `codex(1)`
- `codex(2)`
- `claude(1)`

Collapsed island lights:

- red: running
- yellow: failed / stale
- one green light: idle (no active instances)

## User Setup

1. Open FocuSD Island.
2. Expand the island and open Settings or the Agent page.
3. In **AI Agent 状态灯**, click **安装/修复**.
4. Restart Codex and any VSCode terminal running Claude Code.
5. For Codex, review and trust the new hooks when Codex prompts you, or use `/hooks`.

The installer writes the hook scripts to:

```text
%APPDATA%\com.focusd.island\
```

It then updates:

```text
%USERPROFILE%\.codex\config.toml
%USERPROFILE%\.claude\settings.json
```

Re-running **安装/修复** rewrites managed hooks to the current app-data script path.

## Runtime Contract

Prompt submission creates a per-instance marker quickly:

```text
agent-running-codex-<id>.flag
agent-running-claudeCode-<id>.flag
```

FocuSD polls every ~200ms. Multiple markers mean multiple lights.

When a turn finishes, `focusd-agent-status.ps1`:

1. removes one matching running marker (oldest for that provider if no instance id)
2. updates `agent-status.json`
3. may keep a short hold marker so very short prompts still flash red (~800ms)

`agent-status.json` shape:

```json
{
  "instances": [
    {
      "id": "a1b2c3d4e5f6",
      "provider": "codex",
      "displayIndex": 1,
      "phase": "running",
      "taskId": "optional-id",
      "updatedAt": 1783584000000
    }
  ],
  "updatedAt": 1783584000000
}
```

Legacy single-object files (`codex` / `claudeCode`) and legacy markers
(`agent-codex-running.flag`) are still read for compatibility.

A running marker older than 10 minutes becomes `stale` (yellow).

## Manual Smoke Test

```powershell
$dir = Join-Path $env:APPDATA 'com.focusd.island'
& "$dir\focusd-agent-running.cmd" codex
& "$dir\focusd-agent-running.cmd" codex
& "$dir\focusd-agent-running.cmd" claudeCode
powershell -NoProfile -ExecutionPolicy Bypass -File "$dir\focusd-agent-status.ps1" codex completed
powershell -NoProfile -ExecutionPolicy Bypass -File "$dir\focusd-agent-status.ps1" codex completed
powershell -NoProfile -ExecutionPolicy Bypass -File "$dir\focusd-agent-status.ps1" claudeCode failed
```

Expected:

- two Codex lights after two running starts, plus one Claude light
- each completed stop removes one Codex light
- failed Claude stays as attention until cleared

## Notes

- Prefer the in-app installer.
- On Windows Claude Code hooks, prefer exec form with `cmd.exe` / `powershell.exe` + `args`.
- After upgrading multi-instance support, reinstall hooks once and re-trust Codex hooks.
