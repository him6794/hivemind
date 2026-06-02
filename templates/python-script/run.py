"""Replace this with your own Python script.
Output files go to ./output/ directory.
Print RESULT_TORRENT=<info-hash> for torrent-compatible results.
"""
import json, os, time

os.makedirs("output", exist_ok=True)
with open("output/result.json", "w") as f:
    json.dump({"completed": True, "ts": time.time()}, f, indent=2)
print("Template completed.")
