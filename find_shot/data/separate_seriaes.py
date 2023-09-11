#!/usr/bin/env python


import sys, os
import json


def main():
    seriaes = {}

    # Read file named argv[1]. Use each string as JSON object
    with open(sys.argv[1], "r") as f:
        # Read each line
        for line in f:
            # Read each JSON object
            obj = json.loads(line)
            # Get channel name
            channel_name = obj["channel"]

            # If channel name is not in seriaes, add it
            if channel_name not in seriaes:
                seriaes[channel_name] = [obj["f"]]
            else:
                 # If channel name is in seriaes, append it
                seriaes[channel_name].append(obj["f"])

    # Write seriaes to file named sys.argv[1]-sep.csv
    filename = sys.argv[1] + "-sep.csv"
    with open(filename, "w") as f:
        counter = 0
        while True:
            line = ["" for _ in range(len(seriaes))]
            for serie in seriaes:
                if counter < len(seriaes[serie]):
                    line[serie] = seriaes[serie][counter]

            if line.count("") == len(line):
                break
            f.write(f"{';'.join(map(str, line))}\n")

            counter += 1
            

if __name__ == "__main__":
    main()