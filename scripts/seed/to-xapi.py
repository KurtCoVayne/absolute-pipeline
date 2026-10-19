#!/usr/bin/env python3 -I
"""The seed posts from the mirror's records (scripts/seed/fetch-posts.sh) to
X API v2 response shapes, one file per post, for the `file` adapter:
`{"data": {...tweet...}, "includes": {"users": [...], "tweets": [quoted], "media": [...]}}`
with `created_at`, `entities` (urls with expanded_url, cashtags, hashtags,
mentions), `referenced_tweets`, `conversation_id`, `lang`, `attachments`
and `note_tweet` where the mirror gives them. PL-D7: the content is the
same public post; only the transport differs from the X API's."""
import json, sys, glob, os, datetime

src, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)


def iso(ts):
    return datetime.datetime.fromtimestamp(ts, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


def convert(t, seed=None):
    raw = (t.get("raw_text") or {})
    text = raw.get("text") or t.get("text") or ""
    ents = {"urls": [], "cashtags": [], "hashtags": [], "mentions": []}
    media_keys = []
    for f in raw.get("facets") or []:
        a, b = f.get("indices", [0, 0])
        if f["type"] == "url":
            ents["urls"].append({"start": a, "end": b, "url": f.get("original", ""), "expanded_url": f.get("replacement", ""), "display_url": f.get("display", "")})
        elif f["type"] == "media":
            ents["urls"].append({"start": a, "end": b, "url": f.get("original", ""), "expanded_url": f.get("replacement", ""), "display_url": f.get("display", ""), "media_key": "3_" + str(f.get("id"))})
            media_keys.append("3_" + str(f.get("id")))
        elif f["type"] == "symbol":
            ents["cashtags"].append({"start": a, "end": b, "tag": f.get("original", "")})
        elif f["type"] == "hashtag":
            ents["hashtags"].append({"start": a, "end": b, "tag": f.get("original", "")})
        elif f["type"] == "mention":
            ents["mentions"].append({"start": a, "end": b, "username": f.get("original", ""), "id": str(f.get("id", ""))})
    d = {
        "id": str(t["id"]),
        "text": text,
        "author_id": str(t["author"]["id"]),
        "created_at": iso(t["created_timestamp"]),
        "conversation_id": str(t.get("replying_to_status") or t["id"]),
        "lang": t.get("lang") or "und",
        "entities": {k: v for k, v in ents.items() if v},
        "public_metrics": {"retweet_count": t.get("retweets", 0), "reply_count": t.get("replies", 0), "like_count": t.get("likes", 0), "quote_count": t.get("quotes", 0)},
    }
    if t.get("is_note_tweet"):
        d["note_tweet"] = {"text": text}
    refs = []
    if t.get("replying_to_status"):
        refs.append({"type": "replied_to", "id": str(t["replying_to_status"])})
    if t.get("quote"):
        refs.append({"type": "quoted", "id": str(t["quote"]["id"])})
    if refs:
        d["referenced_tweets"] = refs
    if media_keys:
        d["attachments"] = {"media_keys": media_keys}
    users = [{"id": str(t["author"]["id"]), "username": t["author"]["screen_name"], "name": t["author"].get("name", "")}]
    media = []
    for m in ((t.get("media") or {}).get("all") or []):
        media.append({"media_key": "3_" + str(m.get("id")), "type": m.get("type"), "url": m.get("url") or m.get("thumbnail_url"), "alt_text": m.get("altText")})
    inc = {"users": users}
    if media:
        inc["media"] = media
    if t.get("quote"):
        q = convert(t["quote"])
        inc["tweets"] = [q["data"]]
        inc["users"] += q["includes"]["users"]
    return {"data": d, "includes": inc}


n = 0
for f in sorted(glob.glob(os.path.join(src, "*.json"))):
    rec = json.load(open(f))
    t = rec["tweet"]
    x = convert(t)
    x["seed"] = rec.get("seed")
    with open(os.path.join(out, os.path.basename(f)), "w") as w:
        json.dump(x, w, ensure_ascii=False, indent=1)
    n += 1
print(f"{n} posts written to {out}")
