#!/usr/bin/bin python


from collections.abc import Callable
import random
import torch

from torch.utils.data import IterableDataset

DEFAULT_TARGET = 32764.0
TEST_PERIOD = 3.0  # seconds
UPDATE_PERIOD = 0.1  # seconds


def gen_func(target: float, a: float, pow: float) -> Callable:
    def func(x: float) -> float:
        return target - a / (x + 1) ** pow
    return func


def generate_data_fragment(target: float, a: float, pow: float, period: float, interval: float) -> (list[float], list[float]):
    points_count = int(period / interval)
    func = gen_func(target, a, pow)
    x_mx = [x * interval for x in range(0, points_count)]
    y_mx = [func(x) for x in x_mx]
    return (x_mx, y_mx)


def add_noise(y_mx: list[float], noise: Callable) -> list[float]:
    return [y + noise(y) for y in y_mx]


def move_x(s_data: list[float], offset: float) -> list[float]:
    return [x + offset for x in s_data]


class Config:
    def __init__(self, a: float, pow: float, period: float):
        self.a = a
        self.pow = pow
        self.period = period


def generate_work_block(target: float, cfg: list[Config], interval: float, stiyk: Callable) -> (list[float], list[float], list[bool]):
    x = None
    y = None
    fire = None

    cfg.reverse()
    for block in cfg:
        x_mx, y_mx = generate_data_fragment(
            target, block.a, block.pow, block.period, interval)
        target = y_mx[0]
        if not x:
            x = x_mx
            y = y_mx
            fire = [False for _ in x_mx]
        else:
            move = x[0] - x_mx[-1] - interval
            x = move_x(x_mx, move) + x

            stiyk_y = stiyk(y_mx[-1], y[0])
            y = y_mx[:-1] + stiyk_y + y[1:]
            fire = [False for _ in y_mx[:-1]] + [True] + fire

    return (x, y, fire)


def gen_stik_func(offset: Callable) -> Callable:
    def func(y1: float, y2: float) -> list[float]:
        offset_v = offset(y1, y2)
        return [y1 - offset_v / 2.0, y2 - offset_v]
    return func


def generate_single_fragment(random_styik: Callable, noise_func: Callable, min_point: int, max_poins: int) -> (list[float], list[float], list[bool]):
    # randon tagret less then default_target
    target = DEFAULT_TARGET - random.randrange(0, 500) / 10.0

    x_full, y_full, fire = generate_work_block(target, [
        Config(random.randrange(20, 50) / 90.0,
               random.randrange(30, 50) / 10.0,
               TEST_PERIOD * random.randrange(5, 10) / 10.0) for _ in range(4)
    ], UPDATE_PERIOD, random_styik)
    y_full = add_noise(y_full, noise_func)

    start = random.randrange(0, len(x_full) - min_point)
    stop = random.randrange(start + min_point, len(x_full))
    if stop - start > max_poins:
        stop = start + max_poins

    return (x_full[start:stop], y_full[start:stop], fire[start:stop])


def normalisate_data(x: list[float], y: list[float], fire: list[bool], size: int) -> (list[float], list[float]):
    min_v = min(y)
    max_v = max(y)

    if len(y) <= size:
        first_y = (y[0] - min_v) / (max_v - min_v)
        return ([i for i in range(size)],
                [first_y for _ in range(size - len(y))] +
                [(y_i - min_v) / (max_v - min_v) for y_i in y],
                [0.0 for _ in range(size - len(y))] + [1.0 if fire_i else 0.0 for fire_i in fire])
    else:
        return ([i for i in range(len(x))],
                [(y_i - min_v) / (max_v - min_v) for y_i in y[-size:]],
                [1.0 if fire_i else 0.0 for fire_i in fire[-size:]])


class GeneratedDataset(IterableDataset):
    def generate(self):
        x_full, y_full, fire = generate_single_fragment(
            self.stiyk_func, self.noise_func, int(max(self.size / 2, 5)), self.size)
        _, y, fire = normalisate_data(x_full, y_full, fire, self.size)
        return {'val': torch.tensor(y), 'is_fired': torch.tensor(fire)}
    
    def __init__(self, noise_func: Callable, stiyk_func: Callable, size: int, count: int):
        super().__init__()

        self.noise_func = noise_func
        self.stiyk_func = stiyk_func
        self.size = size
        self.count = count
        
        self.data = [self.generate() for _ in range(self.count)]

    def __iter__(self):
        return iter(self.data)


def main():
    import matplotlib.pyplot as plt

    def noise_func(_y): return random.randrange(-1, 3) / 100.0

    random_styik = gen_stik_func(lambda y1, y2: (
        abs(y1-y2) + abs(noise_func(abs(y1-y2)))) * 10.0)

    SIZE = 20

    dataset = GeneratedDataset(noise_func, random_styik, SIZE, 5)
    x = [i for i in range(SIZE)]

    for item in iter(dataset):
        plt.plot(x, item['val'])
        plt.plot(x, item['is_fired'])
        plt.show()


if __name__ == "__main__":
    main()
