#!/usr/bin/env python

import libSmoothSpline as lss
import sys
import math
import json
import numpy as np
from matplotlib import pyplot as plt
from data.flatterise import normalise, SERIE_SIZE


def build_box_plot(series):
    # медиана
    median = np.median(series)
    # первый квартиль
    q1 = np.percentile(series, 25)
    # третий квартиль
    q3 = np.percentile(series, 75)

    # межквартильный размах
    iqr = q3 - q1

    # нижняя граница
    lower_bound = q1 - 1.5 * iqr
    # верхняя граница
    upper_bound = q3 + 1.5 * iqr

    return median, q1, q3, lower_bound, upper_bound


def filter_serie(serie):
    # smooth serie
    sm = lss.SmoothSpline()

    sm.Points = [lss.Point(i, p) for (i, p) in enumerate(serie)]
    sm.Update(100)

    smooth_serie = []
    for i in range(len(serie)):
        y = sm.Y(i)
        if math.isnan(y):
            smooth_serie.append(serie[i])
        else:
            smooth_serie.append(y)

    diffs = [abs(serie[i] - smooth_serie[i]) for i in range(len(serie))]

    _, _, _, lower_bound, upper_bound = build_box_plot(diffs)

    new_points = []
    for i in range(len(sm.Points)):
        if diffs[i] < lower_bound or diffs[i] > upper_bound:
            pass
        else:
            new_points.append(sm.Points[i])

    sm.Points = new_points
    sm.Update(1)

    filtred_serie = [sm.Y(i) for i in range(len(serie))]
    return smooth_serie, filtred_serie


def main():
    file = sys.argv[1]

    series = []

    with open(file, "r") as f:
        current_seriae = []
        prev_channel = None

        # Read each line
        for line in f:
            # Read each JSON object
            obj = json.loads(line)

            if obj is None:
                continue

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

    serie = series[3]  # 6, 7, 8, 12, 13, 15, 27

    smooth_serie, filtred_serie = filter_serie(serie)
    
    # plot
    plt.plot(serie, marker='o', linestyle='None')
    plt.plot(smooth_serie, color='red')
    plt.plot(filtred_serie, color='green')

    plt.show()


if __name__ == "__main__":
    main()
