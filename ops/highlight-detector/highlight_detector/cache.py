import json
import os

BASIS = os.path.expanduser("~/.cache/highlight-detector")


def vod_dir(vod_id):
    pfad = os.path.join(BASIS, vod_id)
    os.makedirs(pfad, exist_ok=True)
    return pfad


def pfad(vod_id, name):
    return os.path.join(vod_dir(vod_id), name)


def existiert(vod_id, name):
    return os.path.exists(pfad(vod_id, name))


def schreibe_jsonl(vod_id, name, zeilen):
    ziel = pfad(vod_id, name)
    with open(ziel, "w") as f:
        for z in zeilen:
            f.write(json.dumps(z, ensure_ascii=False) + "\n")
    return ziel


def lies_jsonl(vod_id, name):
    ziel = pfad(vod_id, name)
    if not os.path.exists(ziel):
        return []
    out = []
    with open(ziel) as f:
        for zeile in f:
            zeile = zeile.strip()
            if zeile:
                out.append(json.loads(zeile))
    return out


def schreibe_json(vod_id, name, obj):
    ziel = pfad(vod_id, name)
    with open(ziel, "w") as f:
        json.dump(obj, f, ensure_ascii=False)
    return ziel


def lies_json(vod_id, name):
    ziel = pfad(vod_id, name)
    if not os.path.exists(ziel):
        return None
    with open(ziel) as f:
        return json.load(f)
