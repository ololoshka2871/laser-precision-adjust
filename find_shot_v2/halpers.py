import os
import json


class RawPoint:
    def __init__(self, x: float, y: float):
        self.x = x
        self.y = y

    def __repr__(self):
        return f'RawPoint({self.x},{self.y})'


class Fragment:
    def __init__(self, start_timestamp: int, raw_points: list[RawPoint], coeffs: (float, float), min_index: int):
        self.start_timestamp = start_timestamp
        self.raw_points = raw_points
        self.coeffs = coeffs
        self.min_index = min_index

    def __repr__(self):
        return f'Fragment(start={self.start_timestamp}, coeffs={self.coeffs})'


def load_all_series(file: str):
    with open(file, "r") as f:
        fragments = []
        # Read each JSON object
        obj = json.loads(f.read())
        for channel in obj:
            for short in channel:
                raw_pouis = [RawPoint(**rp) for rp in short['raw_points']]
                fragment = Fragment(short['start_timestamp'], raw_pouis, short['coeffs'], short['min_index'])
                fragments.append(fragment)
        return fragments


def load_all_logs_in_folder(folder: str) -> list:
    # найти все файлы .log в папке folder
    files = [f for f in os.listdir(
        folder) if f.endswith('.log-fragments.json')]

    global_fragments = []
    for file in files:
        local_series = load_all_series(os.path.join(folder, file))
        global_fragments.extend(local_series)

    return global_fragments
