"""Step-by-step relay for the persistent visual demo (emu-up.sh running).
Each subcommand is one relay action; UI-gated steps block until someone taps
the device screen (in the browser page or on real glass).

    python3 relay/demo_steps.py cut [title] [edition]
    python3 relay/demo_steps.py pair       # handshake, then SAS on both
    python3 relay/demo_steps.py press
    python3 relay/demo_steps.py verify

Relay state (public byte blobs only) parks in /tmp/presse-relay/."""

import os
import struct
import sys
import threading

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tests"))

import requests  # noqa: E402

from presse_client import (  # noqa: E402
    Presse,
    apdu_hex,
    split_sw,
    sas_words_on_screen,
    verify_chain,
    verify_possession,
    INS_PAIR_COMMIT,
    INS_PAIR_RESPOND,
    INS_PAIR_REVEAL,
    INS_PAIR_FINISH,
    INS_PAIR_SAS,
    INS_GET_ALBUM,
    INS_PRESS_REQUEST,
    INS_PRESS_OFFER,
    INS_PRESS_LOAD_ALBUM,
    INS_PRESS_ACCEPT,
    INS_CUT,
    SW_OK,
)

STATE = "/tmp/presse-relay"


class LiveDevice:
    """Same interface as conftest.SpeculosDevice, but attaches to an already
    running Speculos instead of spawning one."""

    def __init__(self, name, api_port):
        self.name = name
        self.url = f"http://127.0.0.1:{api_port}"

    def apdu(self, hexstr):
        r = requests.post(f"{self.url}/apdu", json={"data": hexstr}, timeout=600)
        r.raise_for_status()
        return r.json()["data"]

    def apdu_async_start(self, hexstr):
        result = {}

        def run():
            result["data"] = self.apdu(hexstr)

        t = threading.Thread(target=run, daemon=True)
        t.start()
        return t, result

    def events(self):
        return requests.get(f"{self.url}/events", timeout=5).json().get("events", [])

    def screen_texts(self):
        return [e.get("text", "") for e in self.events()]

    def wait_for_text(self, needle, timeout=600.0):
        import time

        deadline = time.time() + timeout
        while time.time() < deadline:
            if any(needle in t for t in self.screen_texts()):
                return True
            time.sleep(0.4)
        return False


def blob(name, data=None):
    os.makedirs(STATE, exist_ok=True)
    path = os.path.join(STATE, name)
    if data is None:
        with open(path, "rb") as f:
            return f.read()
    with open(path, "wb") as f:
        f.write(data)


def gated(p, ins, data, wait_text, who):
    t, r = p.dev.apdu_async_start(apdu_hex(ins, data))
    assert p.dev.wait_for_text(wait_text, timeout=10), f"no '{wait_text}' screen"
    print(f">> confirm on {who} (tap the screen in its browser page)")
    t.join(timeout=600)
    return split_sw(r["data"])


def cockpit_running() -> bool:
    try:
        requests.get("http://127.0.0.1:5050/log", timeout=1)
        return True
    except requests.RequestException:
        return False


def main():
    step = sys.argv[1] if len(sys.argv) > 1 else "help"
    if cockpit_running():
        # Route through the cockpit so the wire feed shows the real traffic.
        a = Presse(LiveDevice("flex-a", 5050))
        b = Presse(LiveDevice("flex-b", 5050))
        a.dev.url = "http://127.0.0.1:5050/a"
        b.dev.url = "http://127.0.0.1:5050/b"
    else:
        a = Presse(LiveDevice("flex-a", 5001))
        b = Presse(LiveDevice("flex-b", 5002))

    if step == "cut":
        title = sys.argv[2] if len(sys.argv) > 2 else "Random Access Memories"
        edition = int(sys.argv[3]) if len(sys.argv) > 3 else 5
        data = struct.pack("<H", edition) + title.encode()
        body, sw = gated(a, INS_CUT, data, "Cut master", "Flex A")
        assert sw == SW_OK, f"refused ({sw})"
        print(f'master of "{title}" cut, edition of {edition}: now physics.')

    elif step == "pair":
        commitment = a.cmd(INS_PAIR_COMMIT)
        eb = b.cmd(INS_PAIR_RESPOND, commitment)
        ea = a.cmd(INS_PAIR_REVEAL, eb)
        b.cmd(INS_PAIR_FINISH, ea)
        ta, ra = a.dev.apdu_async_start(apdu_hex(INS_PAIR_SAS))
        tb, rb = b.dev.apdu_async_start(apdu_hex(INS_PAIR_SAS))
        assert a.dev.wait_for_text("Words match", timeout=10)
        assert b.dev.wait_for_text("Words match", timeout=10)
        print(f"Flex A shows: {' / '.join(sas_words_on_screen(a.dev))}")
        print(f"Flex B shows: {' / '.join(sas_words_on_screen(b.dev))}")
        print(">> compare, then tap 'Words match' on BOTH pages")
        ta.join(timeout=600)
        tb.join(timeout=600)
        assert split_sw(ra["data"])[1] == SW_OK and split_sw(rb["data"])[1] == SW_OK, "aborted"
        print("channel authenticated.")

    elif step == "press":
        album_msg = a.cmd(INS_GET_ALBUM)
        req = b.cmd(INS_PRESS_REQUEST)
        cert_mac, sw = gated(a, INS_PRESS_OFFER, req, "Press ", "Flex A")
        assert sw == SW_OK, f"refused ({sw})"
        b.cmd(INS_PRESS_LOAD_ALBUM, album_msg)
        _, sw = gated(b, INS_PRESS_ACCEPT, cert_mac, "Receive ", "Flex B")
        assert sw == SW_OK, f"refused ({sw})"
        print(f"pressed. {a.get_info()['counter']} remain in the master.")

    elif step == "collection":
        target = sys.argv[2] if len(sys.argv) > 2 else "a"
        p = a if target == "a" else b
        print(f">> browse the collection on Flex {target.upper()}; tap Back to leave")
        _, sw = split_sw(p.dev.apdu(apdu_hex(0x02)))
        print("closed." if sw == SW_OK else f"refused ({sw})")

    elif step == "verify":
        pressing = b.cmd(0x40, p1=0)
        album = b.cmd(0x40, p1=1)
        info_b = b.get_info()
        result = verify_chain(album, pressing, info_b["devpub"])
        verify_possession(b, pressing)
        print(f'GENUINE: pressing {result["number"]} of {result["edition"]} of '
              f'"{result["title"]}", bound to Flex B, possession proven live.')

    else:
        print(__doc__)


if __name__ == "__main__":
    main()
