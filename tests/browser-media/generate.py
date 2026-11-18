"""Generate owned media fixtures with the developer GStreamer SDK.

python tests/browser-media/generate.py --sdk <GStreamer SDK> --output <scratch>
No downloaded media, normal browser profile or audio device is used.
"""
import argparse
import ctypes
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    sdk = args.sdk.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    ctypes.windll.kernel32.SetErrorMode(0x8003)
    env = os.environ.copy()
    env["PATH"] = str(sdk / "bin") + os.pathsep + env.get("PATH", "")
    # Scanning the entire developer SDK can probe optional GPU/capture/network
    # plugins. Fixture generation needs only these deterministic file plugins.
    plugins = output / "generator-plugins"
    plugins.mkdir(exist_ok=True)
    for name in ["gstcoreelements", "gsttypefindfunctions", "gstaudiotestsrc", "gstvideotestsrc", "gstvpx", "gstmatroska", "gstopus", "gstaudioconvert", "gstaudioresample", "gstopenh264", "gstvideoparsersbad", "gstisomp4"]:
        shutil.copy2(sdk / "lib/gstreamer-1.0" / (name + ".dll"), plugins / (name + ".dll"))
    env["GST_PLUGIN_PATH_1_0"] = str(plugins)
    env["GST_PLUGIN_PATH"] = ""
    env["GST_PLUGIN_SYSTEM_PATH_1_0"] = ""
    env["GST_REGISTRY"] = str(output / "generator-registry.bin")
    video = ["videotestsrc", "num-buffers=120", "pattern=smpte", "!", "video/x-raw,format=I420,width=160,height=90,framerate=30/1", "!"]
    audio = ["audiotestsrc", "wave=silence", "num-buffers=188", "samplesperbuffer=1024", "!", "audio/x-raw,rate=48000,channels=1", "!", "audioconvert", "!"]
    cases = {
        "video.webm": [*video, "vp8enc", "deadline=1", "!", "webmmux"],
        "audio.webm": [*audio, "opusenc", "!", "webmmux"],
        "video.mp4": [*video, "openh264enc", "!", "h264parse", "!", "mp4mux"],
    }
    for name, color, width, height in [("red", "0xffff0000", 160, 90), ("blue", "0xff0000ff", 160, 90), ("green", "0xff00ff00", 320, 180)]:
        cases[name + ".webm"] = ["videotestsrc", "num-buffers=60", "pattern=solid-color", "foreground-color=" + color, "!", f"video/x-raw,format=I420,width={width},height={height},framerate=30/1", "!", "vp8enc", "deadline=1", "!", "webmmux"]
    for name, pipeline in cases.items():
        target = output / name
        # gst_parse_launch treats backslashes in location as escapes, even when
        # subprocess received a correctly quoted Windows argument vector.
        command = [str(sdk / "bin/gst-launch-1.0.exe"), "-q", *pipeline, "!", "filesink", "location=" + target.as_posix()]
        result = subprocess.run(command, env=env, capture_output=True, timeout=120)
        (output / (name + ".log")).write_bytes(result.stdout + result.stderr)
        if result.returncode or not target.is_file() or target.stat().st_size < 256:
            raise RuntimeError(f"Fixture generation failed: {name}; see its scratch log")
        print(f"{name}: {target.stat().st_size} bytes", flush=True)


if __name__ == "__main__":
    main()
