#!/usr/bin/env python

import sys
import math
import numpy as np
from matplotlib import pyplot as plt
from csaps import ISmoothingSpline, csaps 
from data.flatterise import SERIE_SIZE
from load_serie import load_serie


def build_box_plot(series: list[float]) -> dict:
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

    return {
        'median': median,
        'q1': q1,
        'q3': q3,
        'lower_bound': lower_bound,
        'upper_bound': upper_bound
    }

def denoize_serie(data, smooth):
    dns, sm = create_denoized_spline(data, smooth)

    smooth_serie = [sm.Y(i) for i in range(len(data) - 1)]
    filtred_serie = [dns.Y(i) for i in range(len(data) - 2)]

    return smooth_serie, filtred_serie


def aproximete_spline(data: list[float], smooth: float) -> ISmoothingSpline:
    x = np.linspace(0, len(data) - 1, num=len(data))    
    return csaps(x, data, smooth=smooth)


def create_denoized_spline(data: list[float], smooth: float) -> (ISmoothingSpline, ISmoothingSpline):
    sm = aproximete_spline(data, smooth)

    smooth_serie = np.array([])
    for i in range(len(data)):
        y = sm(i)
        smooth_serie = np.append(smooth_serie, y)

    diffs = np.subtract(smooth_serie, data)
    
    bx = build_box_plot(diffs)
    lower_bound, upper_bound = (bx['lower_bound'], bx['upper_bound'])

    weigths = [1.0] * len(data)
    for index in filter(lambda i: diffs[i] < lower_bound or diffs[i] > upper_bound, range(len(diffs))):
        weigths[index] = 0.01
        
    x = np.linspace(0, len(data) - 1, len(data))  
    dns = csaps(x, data, weights=weigths, smooth=smooth)

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
