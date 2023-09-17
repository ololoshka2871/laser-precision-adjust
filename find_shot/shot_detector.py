
from enum import Enum


class States(Enum):
    WAIT = 0  # Ожидание начала спада
    FALLING = 1  # Спад
    RAIZING = 2  # Рост
        
        
def new_detect_shots(derivative: list[float]) -> list[int]:
    if len(derivative) < 5:
        return []
    
    state = States.WAIT
    start = None
    count = 0
    for i, v in enumerate(derivative):
        if state == States.WAIT:
            if v < 0.0:
                start = i
                count = 1
                state = States.FALLING
        elif state == States.FALLING:
            if v < derivative[i - 1]:
                count += 1
            elif v > 0.0:
                # reset
                state = States.WAIT
            else:
                count += 1
                state = States.RAIZING
        elif state == States.RAIZING:
            if v > derivative[i - 1]:
                count += 1
                continue
            elif v > 0.0 and count > 4:
                yield start

            state = States.WAIT
        else:
            raise Exception('Unknown state')