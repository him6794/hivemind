"""Data processing template.
Expects input files in ./input/ directory.
Writes results to ./output/ directory.
"""
import json, os, csv, time, glob

os.makedirs("output", exist_ok=True)
inputs = glob.glob("input/*")
processed = len(inputs)

with open("output/summary.json", "w") as f:
    json.dump({
        "files_processed": processed,
        "ts": time.time()
    }, f, indent=2)

print(f"Processed {processed} input files.")
