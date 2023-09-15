#!/usr/bin/env python

import libSmoothSpline as lss
import sys
import math
import json
import numpy as np
from matplotlib import pyplot as plt
from data.flatterise import SERIE_SIZE
from load_serie import load_serie


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


def denoize_serie(data, smooth):
    dns, sm = create_denoized_spline(data, smooth)

    smooth_serie = [sm.Y(i) for i in range(len(data) - 1)]
    filtred_serie = [dns.Y(i) for i in range(len(data) - 2)]

    return smooth_serie, filtred_serie


def aproximete_spline(data: list[float], smooth: float):
    sm = lss.SmoothSpline()

    sm.Points = [lss.Point(i, p) for (i, p) in enumerate(data)]
    sm.Update(smooth)

    return sm


def create_denoized_spline(data: list[float], smooth: float) -> (lss.SmoothSpline, lss.SmoothSpline):
    sm = aproximete_spline(data, smooth)

    smooth_serie = []
    for i in range(len(data)):
        y = sm.Y(i)
        if math.isnan(y):
            smooth_serie.append(data[i])
        else:
            smooth_serie.append(y)

    diffs = [abs(s - d)
             for (s, d) in zip(smooth_serie, data)]
    
    _, _, _, lower_bound, upper_bound = build_box_plot(diffs)

    new_points = list(filter(lambda x: x is not None, [
        p if (d > lower_bound) and (d < upper_bound) else None
        for (p, d) in zip(sm.Points, diffs)
    ]))

    dns = lss.SmoothSpline()
    dns.Points = new_points
    dns.Update(smooth)

    return dns, sm


def main():
    file: str = sys.argv[1]
    smooth: float = float(sys.argv[2])
    number: int = int(sys.argv[3])

    serie = load_serie(file, number, SERIE_SIZE)

    smooth_serie, filtred_serie = denoize_serie(serie, smooth)

    # plot
    plt.plot(serie, marker='o', linestyle='None')
    plt.plot(smooth_serie, color='red')
    plt.plot(filtred_serie, color='green')

    plt.show()


if __name__ == "__main__":
    main()
