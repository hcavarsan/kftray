import struct
import subprocess
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "server" / "test_proxy.sh"
QUESTION = b"\x06google\x03com\x00\x00\x01\x00\x01"
ANSWER = b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04\xcb\x00\x71\x05"


def run_function(name: str, data: bytes = b"") -> subprocess.CompletedProcess[bytes]:
    functions = SCRIPT.read_text().rsplit('\nmain "$@"', 1)[0]
    return subprocess.run(
        ["bash", "-c", f"{functions}\n{name}"],
        input=data,
        capture_output=True,
        timeout=5,
        check=False,
    )


def dns_response(
    flags: int = 0x8180, answers: int = 1, transaction: int = 0x1234
) -> bytes:
    payload = struct.pack("!6H", transaction, flags, 1, answers, 0, 0) + QUESTION
    if answers:
        payload += ANSWER
    return struct.pack("!I", len(payload)) + payload


def test_dns_probe_preserves_query_bytes_and_frame_length() -> None:
    result = run_function("send_dns_query")
    assert result.returncode == 0, result.stderr
    frame = result.stdout
    assert struct.unpack("!I", frame[:4])[0] == len(frame[4:]) == 28
    assert struct.unpack("!6H", frame[4:16]) == (0x1234, 0x0100, 1, 0, 0, 0)
    assert frame[16:] == QUESTION


def test_dns_probe_accepts_a_successful_answer() -> None:
    result = run_function("check_dns_response", dns_response())
    assert result.returncode == 0, result.stderr


@pytest.mark.parametrize(
    "frame",
    [
        dns_response(flags=0x8181),
        dns_response(flags=0x8183),
        dns_response(flags=0x0180),
        dns_response(answers=0),
        dns_response(transaction=0x4321),
        dns_response()[:-1],
        b"\x00\x00\x00\x0c\x12\x34\x81",
        b"\x00\x00\x00",
        b"",
    ],
    ids=[
        "format-error",
        "nxdomain",
        "query",
        "no-answer",
        "wrong-id",
        "truncated-body",
        "short-header",
        "short-frame",
        "empty",
    ],
)
def test_dns_probe_rejects_failed_or_truncated_replies(frame: bytes) -> None:
    assert run_function("check_dns_response", frame).returncode != 0
