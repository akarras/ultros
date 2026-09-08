#!/usr/bin/env python3
"""Bounded, read-only board evidence for #1178; never requests a refresh.

Capture twice at least six minutes apart, using the same world and item IDs.
Raw responses stay in the output directory; report.json contains comparisons.
No matching board is proof of websocket delivery or a completed reconciliation.
"""

import argparse
from collections import Counter
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
from urllib.parse import quote
from urllib.request import Request, urlopen


def now():
    return datetime.now(timezone.utc).isoformat()


def capture(url, path):
    envelope = {"url": url, "started_at": now()}
    try:
        request = Request(url, headers={"User-Agent": "ultros-stale-listing-probe/1"})
        with urlopen(request, timeout=30) as response:
            envelope["status"] = response.status
            envelope["headers"] = {
                key: response.headers.get(key)
                for key in ("date", "age", "cache-control", "cf-cache-status",
                            "x-ultros-commit", "content-type")
            }
            body = response.read(8_000_001)
            if len(body) > 8_000_000:
                raise ValueError("response exceeds 8 MB evidence limit")
            envelope["body"] = json.loads(body)
    except Exception as error:
        envelope["error"] = f"{type(error).__name__}: {error}"
    envelope["finished_at"] = now()
    path.write_text(json.dumps(envelope, indent=2) + "\n")
    return envelope


def body(envelope):
    if "error" in envelope:
        raise ValueError(envelope["error"])
    if envelope["status"] != 200:
        raise ValueError(f"HTTP {envelope['status']}")
    return envelope["body"]


def checked_signature(name, city, hq, price, quantity):
    if (not isinstance(name, str) or type(city) is not int
            or type(hq) is not bool or type(price) is not int or price <= 0
            or type(quantity) is not int or quantity <= 0):
        raise ValueError("invalid listing signature")
    # Ultros caches retainer city separately; stale city metadata must not be
    # counted as phantom stock. IDs are not comparable in the public APIs.
    return name, hq, price, quantity


def upstream_board(envelope, item, world_id):
    payload = body(envelope)
    board = payload if "itemID" in payload else payload["items"][str(item)]
    if board["itemID"] != item or board["worldID"] != world_id:
        raise ValueError("Universalis item/world mismatch")
    if board.get("hasData") is False:
        raise ValueError("Universalis hasData=false")
    listings = board["listings"]
    if len(listings) != board["listingsCount"]:
        raise ValueError("Universalis board truncated; cannot compare multisets")
    signatures = Counter(checked_signature(
        row["retainerName"], row["retainerCity"], row["hq"],
        row["pricePerUnit"], row["quantity"],
    ) for row in listings)
    return board, signatures


def ultros_board(envelope, item, world_id):
    board = body(envelope)
    signatures = Counter()
    for row, retainer in board["listings"]:
        if (row["world_id"] != world_id or row["item_id"] != item
                or retainer["world_id"] != world_id):
            raise ValueError("Ultros item/world mismatch")
        signatures[checked_signature(
            retainer["name"], retainer["retainer_city_id"], row["hq"],
            row["price_per_unit"], row["quantity"],
        )] += 1
    return board, signatures


def floors(signatures):
    return {label: min((key[2] for key in signatures if key[1] == quality),
                       default=None)
            for label, quality in (("NQ", False), ("HQ", True))}


def difference(left, right):
    return [{"signature": list(key), "count": count}
            for key, count in sorted((left - right).items())]


def compare(before, ours, after, cheapest, item, world_id):
    try:
        first, first_signatures = upstream_board(before, item, world_id)
        last, last_signatures = upstream_board(after, item, world_id)
        local, local_signatures = ultros_board(ours, item, world_id)
        stable = first_signatures == last_signatures
        report = {
            "item_id": item,
            "state": "stable_bracket" if stable else "moving_upstream_board",
            "universalis_upload_ms": [first["lastUploadTime"], last["lastUploadTime"]],
            "ultros_ingest_markers": local["last_updated"],
            "listing_counts": {"universalis": sum(last_signatures.values()),
                               "ultros": sum(local_signatures.values())},
            "universalis_floor": floors(last_signatures),
            "ultros_floor": floors(local_signatures),
            "ultros_only_candidates": difference(local_signatures, last_signatures),
            "universalis_only_candidates": difference(last_signatures, local_signatures),
            "retainer_city_metadata_difference_count": sum((Counter(
                (retainer["name"], retainer["retainer_city_id"], row["hq"],
                 row["price_per_unit"], row["quantity"])
                for row, retainer in local["listings"]
            ) - Counter(
                (row["retainerName"], row["retainerCity"], row["hq"],
                 row["pricePerUnit"], row["quantity"])
                for row in last["listings"]
            )).values()) if local_signatures == last_signatures else None,
            "ultros_oldest_review": min((row[0]["timestamp"]
                                         for row in local["listings"]), default=None),
            "universalis_oldest_review_seconds": min((row["lastReviewTime"]
                                                       for row in last["listings"]), default=None),
            "recent_sales": {label: sorted(({
                "timestamp": row["timestamp"], "price": row["pricePerUnit"],
                "quantity": row["quantity"],
            } for row in last["recentHistory"] if row["hq"] == quality),
                key=lambda row: row["timestamp"], reverse=True)[:3]
                for label, quality in (("NQ", False), ("HQ", True))},
        }
    except (KeyError, TypeError, ValueError) as error:
        return {"item_id": item, "state": "unavailable", "error": str(error)}
    try:
        selected = [row for row in body(cheapest)["cheapest_listings"]
                    if row["item_id"] == item]
        if any(row["world_id"] != world_id for row in selected):
            raise ValueError("analyzer world mismatch")
        if any(type(row["hq"]) is not bool
               or type(row["cheapest_price"]) is not int
               or row["cheapest_price"] <= 0 for row in selected):
            raise ValueError("invalid analyzer quality or price")
        report["analyzer_floor"] = {}
        for label, quality in (("NQ", False), ("HQ", True)):
            matches = [row["cheapest_price"] for row in selected if row["hq"] == quality]
            if len(matches) > 1:
                raise ValueError("duplicate analyzer quality")
            report["analyzer_floor"][label] = matches[0] if matches else None
    except (KeyError, TypeError, ValueError) as error:
        report["analyzer_error"] = str(error)
        report.pop("analyzer_floor", None)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--world", required=True, help="One world, never a DC or region")
    parser.add_argument("--world-id", required=True, type=int)
    parser.add_argument("--items", required=True, help="Comma-separated positive item IDs, at most 20")
    parser.add_argument("--output", required=True, type=Path, help="New directory for this capture")
    args = parser.parse_args()
    try:
        items = list(dict.fromkeys(int(value) for value in args.items.split(",")))
        if not 1 <= len(items) <= 20 or min(items) <= 0 or args.world_id <= 0:
            raise ValueError()
    except ValueError:
        parser.error("use 1–20 positive item IDs and a positive world ID")
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"started_at": now(), "world": args.world, "world_id": args.world_id,
              "items": items, "method": "Universalis before, analyzer, Ultros boards, Universalis after"}
    report["local_revision"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], text=True,
        cwd=Path(__file__).resolve().parent.parent,
    ).strip()
    world = quote(args.world, safe="")
    url = (f"https://universalis.app/api/v2/{args.world_id}/"
           f"{','.join(map(str, items))}?listings=1000&entries=20")
    before = capture(url, args.output / "universalis-before.json")
    cheapest = capture(f"https://ultros.app/api/v1/cheapest/{world}",
                       args.output / "analyzer.json")
    boards = {item: capture(f"https://ultros.app/api/v1/listings/{world}/{item}",
                            args.output / f"ultros-{item}.json") for item in items}
    after = capture(url, args.output / "universalis-after.json")
    report["production_revisions"] = sorted({envelope.get("headers", {}).get("x-ultros-commit")
                                            or "unknown" for envelope in [cheapest, *boards.values()]})
    report["comparisons"] = [compare(before, boards[item], after, cheapest, item, args.world_id)
                             for item in items]
    report["finished_at"] = now()
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    # This is a capture-completeness exit status, never an issue-closure signal.
    return int(any(row["state"] == "unavailable" or "analyzer_error" in row
                   for row in report["comparisons"]))


if __name__ == "__main__":
    raise SystemExit(main())
