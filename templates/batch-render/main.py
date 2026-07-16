"""Batch render template.
Set first_frame and last_frame to control the frame range (default: 1-10).
Results are reported to stdout by the Rust worker.
"""
first_frame = 1
last_frame = 10
print("Rendered " + str(last_frame - first_frame + 1) + " frames.")
