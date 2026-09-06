import json
import subprocess

import numpy as np

FFMPEG = "/usr/bin/ffmpeg"
FFPROBE = "/usr/bin/ffprobe"


def dauer_s(pfad):
    out = subprocess.run(
        [FFPROBE, "-v", "error", "-show_entries", "format=duration",
         "-of", "default=noprint_wrappers=1:nokey=1", pfad],
        capture_output=True, text=True, check=True,
    )
    return float(out.stdout.strip())


def aufloesung(pfad):
    out = subprocess.run(
        [FFPROBE, "-v", "error", "-select_streams", "v", "-show_entries",
         "stream=width,height", "-of", "csv=p=0", pfad],
        capture_output=True, text=True, check=True,
    )
    w, h = out.stdout.strip().split(",")[:2]
    return int(w), int(h)


def frame_bei(pfad, t, breite, hoehe):
    proc = subprocess.run(
        [FFMPEG, "-v", "error", "-ss", str(t), "-i", pfad, "-frames:v", "1",
         "-pix_fmt", "bgr24", "-f", "rawvideo", "-"],
        capture_output=True, check=True,
    )
    roh = proc.stdout
    if len(roh) < breite * hoehe * 3:
        return None
    return np.frombuffer(roh[: breite * hoehe * 3], dtype=np.uint8).reshape(hoehe, breite, 3)


def frame_strom(pfad, fps, breite, hoehe, start=0.0, ende=None):
    cmd = [FFMPEG, "-v", "error"]
    if start:
        cmd += ["-ss", str(start)]
    if ende is not None:
        cmd += ["-to", str(ende - start if start else ende)]
    cmd += ["-i", pfad, "-vf", f"fps={fps}", "-pix_fmt", "bgr24", "-f", "rawvideo", "-"]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    frame_bytes = breite * hoehe * 3
    i = 0
    try:
        while True:
            roh = proc.stdout.read(frame_bytes)
            if len(roh) < frame_bytes:
                break
            t = start + i / fps
            frame = np.frombuffer(roh, dtype=np.uint8).reshape(hoehe, breite, 3)
            yield t, frame
            i += 1
    finally:
        proc.stdout.close()
        proc.wait()


def lautstaerke_reihe(pfad, start=0.0, ende=None, hz=1000, fenster_s=1.0):
    cmd = [FFMPEG, "-v", "error"]
    if start:
        cmd += ["-ss", str(start)]
    if ende is not None:
        cmd += ["-to", str(ende - start if start else ende)]
    cmd += ["-i", pfad, "-ac", "1", "-ar", str(hz), "-f", "s16le", "-"]
    proc = subprocess.run(cmd, capture_output=True, check=True)
    pcm = np.frombuffer(proc.stdout, dtype=np.int16).astype(np.float32) / 32768.0
    pro_fenster = int(hz * fenster_s)
    if pro_fenster <= 0 or pcm.size == 0:
        return []
    n = pcm.size // pro_fenster
    reihe = []
    for i in range(n):
        block = pcm[i * pro_fenster:(i + 1) * pro_fenster]
        rms = float(np.sqrt(np.mean(block * block)) + 1e-9)
        reihe.append((start + i * fenster_s, rms))
    return reihe


def szenenwechsel(pfad, schwelle=0.4, start=0.0, ende=None):
    cmd = [FFMPEG, "-v", "error"]
    if start:
        cmd += ["-ss", str(start)]
    if ende is not None:
        cmd += ["-to", str(ende - start if start else ende)]
    cmd += ["-i", pfad, "-vf", f"select='gt(scene,{schwelle})',metadata=print",
            "-an", "-f", "null", "-"]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    zeiten = []
    for zeile in proc.stderr.splitlines():
        zeile = zeile.strip()
        if zeile.startswith("pts_time:"):
            try:
                zeiten.append(start + float(zeile.split(":", 1)[1]))
            except ValueError:
                pass
    return zeiten


def schneide(pfad, start_s, ende_s, ziel):
    cmd = [
        FFMPEG, "-v", "error", "-y", "-ss", str(start_s), "-to", str(ende_s),
        "-i", pfad, "-c:v", "libx264", "-preset", "fast", "-crf", "20",
        "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart", ziel,
    ]
    subprocess.run(cmd, check=True, capture_output=True)
    return ziel


def lade_clip_fenster(url, ziel, timeout=300):
    cmd = [
        "/home/nathanael/.local/bin/yt-dlp", "--no-warnings", "-o", ziel,
        "--merge-output-format", "mp4", url,
    ]
    subprocess.run(cmd, check=True, capture_output=True, timeout=timeout)
    return ziel
