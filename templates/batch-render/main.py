"""Batch render template.
Set RENDER_FRAMES env var to control frame range (default: 1-10).
Output frames go to ./output/ directory.
"""
import json, os, time

os.makedirs("output", exist_ok=True)
frames = os.environ.get("RENDER_FRAMES", "1-10")
start, end = (int(x) for x in frames.split("-"))

for frame in range(start, end + 1):
    # Replace with actual render command
    with open(f"output/frame_{frame:04d}.txt", "w") as f:
        f.write(f"Frame {frame} rendered at {time.time()}\n")

with open("output/manifest.json", "w") as f:
    json.dump({"frames_rendered": end - start + 1, "range": frames}, f, indent=2)

print(f"Rendered {end - start + 1} frames.")
