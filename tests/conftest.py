import os
import subprocess
import time

import pytest
import requests

APP_ELF = os.environ.get(
    "APP_ELF",
    "/mnt/c/Users/sylve/projects/presse/device-app/target/flex/release/app-boilerplate-rust",
)

# API ports are overridable so a run can dodge a Speculos already bound to the
# defaults (e.g. a persistent emu-up, or another worktree's suite in parallel).
PORT_A = int(os.environ.get("SPECULOS_PORT_A", "5001"))
PORT_B = int(os.environ.get("SPECULOS_PORT_B", "5002"))


class SpeculosDevice:
    """One Speculos instance driven over its REST API. Deliberately not Ragger:
    we need two of these in one test."""

    def __init__(self, name: str, api_port: int, elf: str = APP_ELF):
        self.name = name
        self.api_port = api_port
        self.url = f"http://127.0.0.1:{api_port}"
        try:
            requests.get(f"{self.url}/events", timeout=1)
            raise RuntimeError(
                f"port {api_port} already serving a Speculos (emu-up.sh running?). "
                "Stop it with scripts/emu-down.sh before running tests."
            )
        except requests.RequestException:
            pass
        self.proc = subprocess.Popen(
            [
                "speculos", "--model", "flex", "--display", "headless",
                "--api-port", str(api_port), "--apdu-port", "0", elf,
            ],
            stdout=open(f"/tmp/speculos-{name}.log", "wb"),
            stderr=subprocess.STDOUT,
        )
        self._wait_ready()

    def _wait_ready(self, timeout=30.0):
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                requests.get(f"{self.url}/events", timeout=2)
                return
            except requests.RequestException:
                time.sleep(0.3)
        raise RuntimeError(f"{self.name}: speculos API never came up")

    def apdu(self, hexstr: str) -> str:
        """Send an APDU, return full hex response (data + status word)."""
        r = requests.post(f"{self.url}/apdu", json={"data": hexstr}, timeout=30)
        r.raise_for_status()
        return r.json()["data"]

    def apdu_async_start(self, hexstr: str):
        """For APDUs that block on a UI screen: fire in a thread."""
        import threading

        result = {}

        def run():
            result["data"] = self.apdu(hexstr)

        t = threading.Thread(target=run, daemon=True)
        t.start()
        return t, result

    def events(self) -> list:
        r = requests.get(f"{self.url}/events", timeout=5)
        return r.json().get("events", [])

    def screen_texts(self) -> list:
        return [e.get("text", "") for e in self.events()]

    def wait_for_text(self, needle: str, timeout=15.0) -> bool:
        deadline = time.time() + timeout
        while time.time() < deadline:
            if any(needle in t for t in self.screen_texts()):
                return True
            time.sleep(0.3)
        return False

    def finger(self, x: int, y: int):
        requests.post(
            f"{self.url}/finger",
            json={"x": x, "y": y, "action": "press-and-release"},
            timeout=5,
        )

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()


@pytest.fixture
def device():
    d = SpeculosDevice("solo", PORT_A)
    yield d
    d.stop()


@pytest.fixture
def pair():
    a = SpeculosDevice("alpha", PORT_A)
    b = SpeculosDevice("beta", PORT_B)
    yield a, b
    a.stop()
    b.stop()
