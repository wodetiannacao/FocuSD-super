"""FocuSD Agent 多实例 Hook 集成测试（仅使用 Python 标准库）。"""

from __future__ import annotations

import re
import json
import os
import subprocess
from datetime import datetime
from pathlib import Path


PROJECT_DIR = Path(__file__).resolve().parent.parent
STATUS_SCRIPT = PROJECT_DIR / "scripts" / "focusd-agent-status.ps1"
ARTIFACT_ROOT = PROJECT_DIR / "test-artifacts" / "agent-status"


def expected_instance_id(session_id: str) -> str:
    """与 PowerShell 脚本保持一致：替换 Windows 文件名不安全字符。"""

    safe_id = re.sub(r"[^a-zA-Z0-9._-]", "-", session_id)
    return f"session-{safe_id}"


def powershell_literal(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"

def invoke_hook(
    status_path: Path,
    provider: str,
    phase: str,
    session_id: str,
    turn_id: str,
) -> dict:
    """向状态脚本 stdin 发送真实 Hook 形状的 JSON，并返回状态文件内容。"""

    payload = json.dumps(
        {
            "session_id": session_id,
            "turn_id": turn_id,
            "cwd": str(PROJECT_DIR),
            "hook_event_name": "UserPromptSubmit" if phase == "running" else "Stop",
        },
        ensure_ascii=False,
    )
    script_command = (
        "[Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false; "
        "$hookInput = [Console]::In.ReadToEnd() | ConvertFrom-Json; "
        "$sessionId = [string]$hookInput.session_id; "
        "$instanceId = if ($sessionId) { 'session-' + ($sessionId -replace '[^a-zA-Z0-9._-]', '-') } else { '' }; & "
        f"{powershell_literal(str(STATUS_SCRIPT))} "
        f"{powershell_literal(provider)} {powershell_literal(phase)} $instanceId ([string]$hookInput.turn_id) "
        f"-StatusPath {powershell_literal(str(status_path))} "
        # 测试关闭800毫秒最短运行保持，便于立即断言 completed 状态。
        "-MinimumRunningVisibleMs 0 -HookResponse"
    )
    command = [
        "powershell.exe",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script_command,
    ]
    result = subprocess.run(
        command,
        input=payload,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
        env=os.environ.copy(),
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"Hook failed ({provider}/{phase}): {result.stderr or result.stdout}"
        )

    return json.loads(status_path.read_text(encoding="utf-8-sig"))


def by_session(state: dict) -> dict[str, dict]:
    return {item["id"]: item for item in state.get("instances", [])}


def main() -> None:
    run_dir = ARTIFACT_ROOT / datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    run_dir.mkdir(parents=True, exist_ok=False)
    status_path = run_dir / "agent-status.json"

    session_a = "codex-session-A"
    session_b = "codex-session-B"
    id_a = expected_instance_id(session_a)
    id_b = expected_instance_id(session_b)

    state = invoke_hook(status_path, "codex", "running", session_a, "turn-A1")
    state = invoke_hook(status_path, "codex", "running", session_b, "turn-B1")
    instances = by_session(state)
    assert set(instances) == {id_a, id_b}, instances
    assert sorted(item["displayIndex"] for item in instances.values()) == [1, 2]
    assert all(item["phase"] == "running" for item in instances.values())

    # 模拟中断后继续对话：同一 B 会话即使 turn_id 改变，也必须复用原实例，
    # 原红灯继续代表该会话，不能留下幽灵红灯或新增第三个灯。
    state = invoke_hook(status_path, "codex", "running", session_b, "turn-B2")
    assert len(state["instances"]) == 2, state
    instances = by_session(state)
    assert instances[id_b]["displayIndex"] == 2, instances
    assert instances[id_b]["phase"] == "running", instances

    # A 完成后仍保留为 completed，B 必须继续保持 running。
    state = invoke_hook(status_path, "codex", "completed", session_a, "turn-A1")
    instances = by_session(state)
    assert instances[id_a]["phase"] == "completed", instances
    assert instances[id_b]["phase"] == "running", instances

    # B 完成时不能覆盖 A；两个完成状态分别保留，交由前端按分钟设置过滤。
    state = invoke_hook(status_path, "codex", "completed", session_b, "turn-B2")
    instances = by_session(state)
    assert instances[id_a]["phase"] == "completed", instances
    assert instances[id_b]["phase"] == "completed", instances

    print(
        json.dumps(
            {
                "result": "passed",
                "artifactDirectory": str(run_dir),
                "instances": state["instances"],
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()


# 编号1：新增
# 主要修改内容：模拟两个 Codex session_id 的运行、重复事件和分别完成流程。
# 修改目的：自动验证并发 Agent 不合并、不互相完成，且 completed 状态能够延时显示。

# 编号2：新增/修改
# 主要修改内容：增加同一 session_id 更换 turn_id 后仍保持原编号、原运行灯且实例总数不变的断言。
# 修改目的：覆盖中断后继续对话产生重复灯和幽灵红灯的回归场景。

# 编号3：修改
# 主要修改内容：测试由顶层 powershell -Command 按 UTF-8 解析 stdin，只把安全化 instanceId 与 turn_id 传给状态脚本。
# 修改目的：复现正式 Codex/Claude Hook 的 Windows 入口，防止编码或参数绑定差异导致随机实例 ID。
