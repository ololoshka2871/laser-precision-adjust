#!/usr/bin/env python


import sys
import os
import json
import numpy as np


def main():
    file = sys.argv[1]

    with open(file, "r") as f:
        series = json.load(f)

    arr = np.array(series)
        
    # transpose array arr
    arr = arr.T

    # write arr to file
    np.savetxt(sys.stdout, arr, delimiter=";", fmt="%.4f")


if __name__ == "__main__":
    main()