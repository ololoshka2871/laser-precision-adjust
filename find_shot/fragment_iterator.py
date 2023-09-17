from pyparsing import Iterator

from denoize import create_denoized_spline
from shot_detector import new_detect_shots


def next_shot(iterator: list[int]) -> int | None:
    try:
        return next(iterator)
    except StopIteration:
        return None


def fragment_iterator(iterator: Iterator[float], smooth: float = 1.0, window_size: int = 100) -> list[(int, list[float])]:
    """
    :param iterator: итератор - источник данных
    :param smooth: параметр сглаживания
    :param window_size: размер окна
    """
    block = []
    points_counter = 0
    start = None
    last_found_data = None
    while True:
        try:
            # Читаем данные пока не наполнится окно
            while len(block) < window_size:
                point = next(iterator)
                block.append(point)
                points_counter += 1
        except StopIteration:
            # Если данные закончились и нет старта, то выходим
            if start is not None:
                yield (start, last_found_data)
                
            break
            
        # аппроксимируем сплайном
        dns, _ = create_denoized_spline(block, smooth)

        # производная
        derivative_block = [dns(x, 1) for x in range(len(block))]
        
        shot_it = new_detect_shots(derivative_block)
        
        # Есть-ли выстрел в окне?
        first_shot = next_shot(shot_it)
        if first_shot is None:
            # Выстрела нет
            
            if start is not None:
                # возвращаем "хвост"
                yield (start, last_found_data)
                start = None
                        
            # Сдвигаем окно
            block = block[window_size // 2:]
        else:
            # Выстрел есть
            # надо найти где заканчивается "хвост"
            # хвост может закончится еще в это окне или длиться дальше
            
            start = points_counter - len(block) + first_shot
            
            # Есть-ли еще выстрел в окне?
            second_shot = next_shot(shot_it)

            if second_shot is None:
                # Второго выстрела нет
                
                # сохранить данные от first_shot до конца окна
                x = [x for x in range(first_shot, len(block))]
                last_found_data = [dns(x) for x in x]
                
                # Сдвигаем окно
                step = 1
                for x in range(first_shot, len(block)):
                    if derivative_block[x] > 0:
                        break
                    step += 1
                block = block[step:]
            else:
                # Второй выстрел есть

                # вычислить значение сплайна от first_shot до second_shot
                x = [x for x in range(first_shot, second_shot)]
                y = [dns(x) for x in x]
                yield (start, y)
                
                start = points_counter - len(block) + second_shot
                # сохранить данные от second_shot до конца окна
                x = [x for x in range(second_shot, len(block))]
                last_found_data = [dns(x) for x in x]
                
                # Сдвигаем окно
                block = block[second_shot:]
                