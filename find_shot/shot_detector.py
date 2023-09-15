
def detect_raizing(data: list[float], uper_bound: float) -> int:
    current = data[0]
    for i in range(1, len(data)):
        if data[i] > uper_bound:  # Верхний предел превышен - найден конец роста
            return i
        elif data[i] > current:  # Запоминаем текущее значение как максимум, рост продолжается
            current = data[i]
        else:
            return None  # был рост но предел не превышен, значит это ложный сигнал
        

def detect_shots(derivative: list[float], lower_bond: float, upper_bound: float) -> list[int]:
    if len(derivative) < 5:
        return []
    
    i = 0
    while i < len(derivative) - 2:
        if derivative[i] < lower_bond:
            raizing_end = detect_raizing(derivative[i:], upper_bound)
            if raizing_end != None:
                yield i
                i += raizing_end

        i += 1