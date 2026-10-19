"""The analyst reads what it lacked: the post's images, the linked page and the
store's data map (the seed campaign's reread)."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from typing import Any

import pytest

from agents_harness.envelope import MAX_IMAGES, image_parts
from agents_harness.job import load_inputs
from agents_harness.runner import render_instructions, user_input

from .conftest import CANDIDATE_TEXT, build_job, doc
from .fake_openai import Message

ROOT = Path(__file__).resolve().parents[1]


def _rerun() -> Any:
    spec = importlib.util.spec_from_file_location("rerun_analysts", ROOT / "direct" / "rerun_analysts.py")
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


IMAGES = [
    {"url": "https://pbs.twimg.com/media/chart1.jpg", "kind": "photo", "detail": "high"},
    {"url": "https://pbs.twimg.com/media/chart2.jpg", "kind": "photo", "detail": "low"},
]
DATA_MAP = "# The store's data map\n\n**Futures**: BTC (2017-12-18) ES (2010-06-07) NQ (2010-06-07)"


def _with_extras(tmp_path: Path, stage: str = "analyze") -> Any:
    built = build_job(tmp_path, stage, [doc("post-1", "Gold breakout setup, see the chart. https://t.co/x")])
    d0 = built.docs[0]["id"]
    inp = built.job_dir / "in"
    (inp / "docs" / f"{d0}.images.json").write_text(json.dumps(IMAGES + [{"url": "http://insecure/x.jpg", "kind": "photo"}]))
    (inp / "data_map.md").write_text(DATA_MAP)
    return built


def test_the_loader_reads_images_and_the_data_map_without_taking_them_for_documents(tmp_path: Path) -> None:
    built = _with_extras(tmp_path)
    inputs = load_inputs(built.job_dir)
    d0 = built.docs[0]["id"]
    assert list(inputs.docs) == [d0]
    assert [i["url"] for i in inputs.images[d0]] == [i["url"] for i in IMAGES]  # the http:// one is dropped
    assert inputs.data_map == DATA_MAP


def test_images_become_input_parts_after_the_text_and_the_map_ends_the_instructions(tmp_path: Path) -> None:
    built = _with_extras(tmp_path)
    inputs = load_inputs(built.job_dir)
    items = user_input("the text", inputs)
    assert isinstance(items, list) and items[0]["role"] == "user"
    parts = items[0]["content"]
    assert parts[0] == {"type": "input_text", "text": "the text"}
    imgs = [p for p in parts if p["type"] == "input_image"]
    assert [(p["image_url"], p["detail"]) for p in imgs] == [(IMAGES[0]["url"], "high"), (IMAGES[1]["url"], "low")]
    framing = [p["text"] for p in parts if p["type"] == "input_text"][1:]
    assert "untrusted" in framing[0] and framing[-1] == "</images>"
    instructions = render_instructions(inputs.definition, inputs.skills, inputs, "2026-09-18T19:12:37Z")
    assert instructions.rstrip().endswith(DATA_MAP.splitlines()[-1])
    assert instructions.index("<!-- skill house-rules") < instructions.index("the store's data map")


def test_no_images_keeps_the_plain_text_input(tmp_path: Path) -> None:
    built = build_job(tmp_path, "analyze", [doc("post-1", CANDIDATE_TEXT)])
    assert user_input("plain", load_inputs(built.job_dir)) == "plain"


def test_image_parts_are_capped_and_default_to_low() -> None:
    many = [{"url": f"https://example.com/{i}.jpg"} for i in range(9)]
    parts = image_parts("d", many)
    assert sum(1 for p in parts if p["type"] == "input_image") == MAX_IMAGES
    assert {p["detail"] for p in parts if p["type"] == "input_image"} == {"low"}
    assert image_parts("d", []) == []


def test_the_analyst_sees_the_images_and_the_data_map(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = _with_extras(tmp_path)
    fake_openai.script = [Message(json.dumps({"hypotheses": [], "notes": "The chart shows no rule."}))]
    assert run_harness(built) == 0
    req = fake_openai.requests[0]
    sent = [p for it in req.body["input"] if isinstance(it, dict) and isinstance(it.get("content"), list) for p in it["content"]]
    assert [p.get("image_url") for p in sent if p.get("type") == "input_image"] == [i["url"] for i in IMAGES]
    assert "the store's data map" in req.instructions and "BTC (2017-12-18)" in req.instructions


def test_triage_sees_the_images_too(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = _with_extras(tmp_path, "triage")
    fake_openai.script = [Message(json.dumps({"verdict": "commentary", "instruments": ["GC"], "topics": ["breakout", "from image"], "score": 0.2}))]
    assert run_harness(built) == 0
    req = fake_openai.requests[0]
    assert any(p.get("type") == "input_image" for it in req.body["input"] if isinstance(it, dict) and isinstance(it.get("content"), list) for p in it["content"])


def test_the_data_map_names_the_staged_roots_and_what_is_not_held() -> None:
    m = _rerun()
    continuous = [["3898", "2026-08-14T00:00:00Z", "3912", "0.0"], ["2696", "2026-08-14T00:00:00Z", "2700", "0.0"]]
    tickers = [["3898", "2010-06-07T00:01:00Z", "ES", ""], ["468", "2014-01-02T00:00:00Z", "ES", ""], ["2696", "2017-12-18T00:01:00Z", "BTC", ""], ["9", "2014-01-02T00:00:00Z", "AAPL", ""]]
    roots = m.roots_from_rows(continuous, tickers)
    assert roots == [("BTC", "2017-12-18"), ("ES", "2010-06-07")]  # the equity ES (468) and AAPL are no roots
    text = m.render_data_map(roots)
    assert "BTC (2017-12-18) ES (2010-06-07)" in text
    assert "CFTC Commitments of Traders" in text and "→ BTC (the CME Bitcoin future)" in text
    assert "never call it outside the data" in text


def test_images_of_takes_photos_and_thumbnails_with_the_detail_heuristic() -> None:
    m = _rerun()
    mirror = {
        "tweet": {
            "media": {"all": [{"type": "photo", "url": "https://p/1.jpg"}, {"type": "video", "url": "https://v/1.mp4", "thumbnail_url": "https://v/1.jpg"}, {"type": "photo", "url": "http://p/2.jpg"}]}
        }
    }
    short = m.images_of(mirror, "A trend is building https://t.co/abc")
    assert short == [{"url": "https://p/1.jpg", "kind": "photo", "detail": "high"}, {"url": "https://v/1.jpg", "kind": "video thumbnail", "detail": "low"}]
    long_text = "I went to the beach with my family today and had a wonderful time, the weather was great and the kids loved it. " * 2
    assert m.images_of(mirror, long_text)[0]["detail"] == "low"


@pytest.mark.parametrize(
    ("text", "only"),
    [
        ("https://x.com/i/article/2110013208786300928", True),
        ("Read this https://example.com/a", True),
        ("A long post about breakouts on ES with a link https://example.com/a and more words here", False),
        ("no link at all", False),
    ],
)
def test_link_only_posts(text: str, only: bool) -> None:
    assert _rerun().is_link_only(text) is only


def test_the_fetcher_refuses_private_hosts_and_plain_http() -> None:
    m = _rerun()
    for host in ("127.0.0.1", "localhost", "10.0.0.1", "169.254.169.254"):
        assert m.public_host(host) is False
    with pytest.raises(ValueError):
        m.http_text("http://example.com/")
    with pytest.raises(ValueError):
        m.http_text("https://127.0.0.1/")
    assert m.html_to_text("<html><script>x()</script><p>Rule: buy &amp; hold</p></html>") == "Rule: buy & hold"
