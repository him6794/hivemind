#!/bin/sh
# Docker job template - replace with your container commands
echo "Running docker job..."
mkdir -p output
echo '{"status":"completed","type":"docker"}' > output/result.json
echo "Docker job done."
