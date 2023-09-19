import os

from matplotlib import pyplot as plt
from matplotlib.figure import Figure

import numpy as np

from shot_detector import new_detect_shots
from load_serie import load_all_series
from denoize import create_denoized_spline
from shot_detector import States


def load_all_logs_in_folder(folder: str) -> list:
    # найти все файлы .log в папке folder
    files = [f for f in os.listdir(folder) if f.endswith('.log')]

    global_series = []
    for file in files:
        local_series = load_all_series(os.path.join(folder, file))
        
        count = 0
        for s in filter(lambda s: len(s) > 20, local_series):
            if s[0] != s[-1]:
                global_series.append(s)
                count += 1
    
    return global_series


def describe_fragment(serie: list[float], smooth: float = 0.1) -> (Figure, np.ndarray):
    dns, _s = create_denoized_spline(serie, smooth)

    x = np.linspace(0, len(serie) - 1, len(serie))
    values_serie = np.array([dns(x) for x in x])
    derivate_serie = np.array([dns(x, 1) for x in x])

    shots = list(new_detect_shots(derivate_serie))
    yshots = [serie[i] for i in shots]

    fig, ax = plt.subplots(2, 1)
    ax[0].plot(serie, 'b.:', label='orignal values')
    ax[0].plot(values_serie, 'r-', label='smoothed values')
    ax[0].plot(shots, yshots, 'gd', label='shots')
    ax[1].plot(derivate_serie, 'r.--',
             [0] * len(derivate_serie), 'b--')

    return fig, ax


def trimm_fragment_filter(fragment: list[dict], smooth: float = 0.85) -> bool:
    data = fragment['y_origin']
    dns, _ = create_denoized_spline(data, smooth)
    derivative = [dns(x, 1) for x in range(len(data))]

    state: States = States.WAIT
    
    start = -1
    for x, dy in enumerate(derivative):
        if state == States.WAIT:
            if dy < 0:
                state = States.FALLING
                start = x
        elif state == States.FALLING:
            if dy > 0:
                state = States.RAIZING
        elif state == States.RAIZING:
            if dy < 0:
                fragment['start'] = start
                fragment['end'] = x
                return x - start > 15
            
    return False


#  Функция, которой проксимируем.
#  A - цель роста, B - скорость роста
def f_aprox(x: float, A: float, B: float) -> float:
    return A * (1 - np.exp(-B * x))