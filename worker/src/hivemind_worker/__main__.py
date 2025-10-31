"""Module entrypoint for `python -m hivemind_worker`.

Starts the worker node using the refactored worker_node entrypoint.
"""

from .worker_node import run_worker_node

if __name__ == "__main__":
    run_worker_node()
