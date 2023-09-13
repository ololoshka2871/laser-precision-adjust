#!/usr/bin/env python


import sys
import os
import json

SERIE_SIZE = 100


def normalise(serie):
    min_value = min(serie)
    max_value = max(serie)
    diff = max_value - min_value
    if diff == 0:
        return [0 for _ in serie]
    else:
        return [(value - min_value) / diff for value in serie]


def main():
    files = sys.argv[1:]
    series = []

    for file in files:
        with open(file, "r") as f:
            current_seriae = []
            prev_channel = None

            # Read each line
            for line in f:
                # Read each JSON object
                obj = json.loads(line)
                # Get channel name
                channel_name = obj["channel"]

                # If channel name is not in seriaes, add it
                if (prev_channel and channel_name != prev_channel) or len(current_seriae) == SERIE_SIZE:
                    if len(current_seriae) == SERIE_SIZE:
                        s = normalise(current_seriae)
                        series.append(s)

                    current_seriae = [obj["f"]]
                    prev_channel = channel_name
                elif not prev_channel:
                    current_seriae = [obj["f"]]
                    prev_channel = channel_name
                else:
                    current_seriae.append(obj["f"])

    json.dump(series, sys.stdout)


if __name__ == "__main__":
    main()
