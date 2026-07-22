"""M2: the app boots in Speculos and answers APDUs; screens are readable."""

from presse_client import Presse, apdu_hex, split_sw, INS_COLLECTION, INS_GET_INFO, SW_OK


def test_get_info(device):
    body, sw = split_sw(device.apdu(apdu_hex(INS_GET_INFO)))
    assert sw == SW_OK
    p = Presse(device)
    info = p.get_info()
    assert not info["has_master"]
    assert not info["has_pressing"]
    assert len(info["devpub"]) == 65
    assert info["devpub"][0] == 0x04


def test_device_identity_is_stable(device):
    p = Presse(device)
    assert p.get_info()["devpub"] == p.get_info()["devpub"]


def test_collection_empty(device):
    p = Presse(device)
    thread, result = device.apdu_async_start(apdu_hex(INS_COLLECTION))
    assert device.wait_for_text("Empty"), device.screen_texts()
    p.tap_text("Back")
    thread.join(timeout=30)
    assert split_sw(result["data"])[1] == SW_OK


def test_home_screen_readable(device):
    assert device.wait_for_text("Enclave Records"), device.screen_texts()


def test_dual_instances_distinct_identities(pair):
    a, b = pair
    pa, pb = Presse(a), Presse(b)
    assert pa.get_info()["devpub"] != pb.get_info()["devpub"]
