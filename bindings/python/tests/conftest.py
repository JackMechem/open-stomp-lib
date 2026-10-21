from pathlib import Path

import pytest

from open_stomp import capture

LOGS = Path(__file__).resolve().parents[3] / "captures" / "logs"


@pytest.fixture(scope="session")
def main_log() -> Path:
    return LOGS / "01-main-session.log"


@pytest.fixture(scope="session")
def main_session(main_log):
    return capture.parse(main_log.read_text())
