# TASK TEMPLATES

## OVERVIEW
Templates are user-facing task packages and define the input/runtime contract for example workloads.

## RULES
Keep `task.json`, runner, and requirements synchronized. Avoid hidden network/filesystem assumptions. Validate task IDs and resource declarations at the service boundary.