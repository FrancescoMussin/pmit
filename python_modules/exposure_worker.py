"""Line-based JSON worker for sentence-BERT exposure scoring.

Protocol (stdin -> stdout, one JSON per line):
Input:  {"trades": [{"title": "...", "outcome": "..."}, ...]}
Output: {"scores": [0.12, 0.93, ...]}
"""
# for encoding and decoding JSON messages
import json
import os
# we need this for reading environment variables for configuration
# silence HuggingFace and Transformers noise
os.environ["HF_HUB_DISABLE_SYMLINKS_WARNING"] = "1"
os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] = "1"
os.environ["TRANSFORMERS_VERBOSITY"] = "error"
os.environ["TOKENIZERS_PARALLELISM"] = "false"

# to interact with the worker process's stdin and stdout
import sys
import logging
import warnings

# suppress HF Hub warnings about unauthenticated requests
warnings.filterwarnings("ignore", message=".*unauthenticated requests to the HF Hub.*")

# suppress all logging from third-party ML libraries
logging.getLogger("transformers").setLevel(logging.ERROR)
logging.getLogger("sentence_transformers").setLevel(logging.ERROR)
logging.getLogger("huggingface_hub").setLevel(logging.ERROR)

# globally disable tqdm progress bars before importing ML modules
try:
    from tqdm import tqdm
    from functools import partialmethod
    tqdm.__init__ = partialmethod(tqdm.__init__, disable=True)
except ImportError:
    pass

# Any allows any tiype for better type hints in dynamic contexts
from typing import Any

from exposure_scorer import ExposureScorer


def main() -> int:
    # get the temperature from the environment variable, with a default of 0.30 if not set or invalid
    temperature_raw = os.environ.get("EXPOSURE_TEMPERATURE", "0.30")
    try:
        temperature = float(temperature_raw)
    except ValueError:
        temperature = 0.30
    if temperature <= 0.0:
        temperature = 0.30
    # initialize the exposure scorer with the specified temperature.
    scorer = ExposureScorer(temperature=temperature)
    # signal readiness to the parent process by writing a JSON message to stdout.
    sys.stdout.write('{"ready": true}\n')
    # flush to ensure the message is sent immediately and the parent process can proceed.
    sys.stdout.flush()

    # We enter a loop where we read lines from stdin, expecting each line to be a JSON message containing a list of trades.
    for line in sys.stdin:
        # strip whitespace from the line
        line = line.strip()
        # skip if it's empty.
        if not line:
            continue

        try:
            # we parse the JSON message into a Python dictionary.
            # We expect the JSON message to have a string key ("trades") that maps 
            # to a list of trade objects
            payload: dict[str, Any] = json.loads(line)
            # we extract the list of trades from the payload, defaulting to an empty list if the key is missing or not a list.
            trades = payload.get("trades", [])

            # we prepare pairs of (title, outcome) for each trade, ensuring that we handle missing or non-string values gracefully 
            # by converting them to strings and defaulting to empty strings if necessary.
            pairs = []
            for trade in trades:
                title = str(trade.get("title") or "")
                outcome = str(trade.get("outcome") or "")
                pairs.append((title, outcome))

            # we score the batch of trades
            scores = scorer.score_trades_batch(pairs) if pairs else []
            sys.stdout.write(json.dumps({"scores": scores}) + "\n")
            sys.stdout.flush()
        except Exception as exc:
            # check if payload exists and has "trades" key. If so, we use its length for the fallback scores; otherwise, 
            # we default to 0 to avoid crashing the worker.
            fallback_len = len(payload.get("trades", [])) if "payload" in locals() else 0
            # we log the error to stderr so that we see it in the console or logs, but we don't want to crash the worker process. 
            sys.stderr.write(f"[exposure_worker] request error: {exc}\n")
            sys.stderr.flush()
            # write that we had an error but keep the worker alive by returning a fallback response with scores of 0.0 for each trade 
            # in the request, ensuring that the length of the scores list matches the number of trades we received (if we were able to parse it).
            sys.stdout.write(json.dumps({"scores": [0.0] * fallback_len}) + "\n")
            sys.stdout.flush()

    return 0

if __name__ == "__main__":
    raise SystemExit(main())
