from pyparsing import Iterator

from denoize import create_denoized_spline
from shot_detector import new_detect_shots


def next_shot(iterator: list[int]) -> int | None:
    try:
        return next(iterator)
    except StopIteration:
        return None


def fragment_iterator(iterator: Iterator[float], smooth: float = 1.0, window_size: int = 100) -> list[(int, list[float])]:  # list[dict]
    """
    :param iterator: итератор - источник данных
    :param smooth: параметр сглаживания
    :param window_size: размер окна
    """
    block = []
    points_counter = 0
    start = None
    last_found_smooth_data = None
    last_found_smooth_data_origin = None
    last_found_spline = None
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
                yield {'start': start, 'smooth_data': last_found_smooth_data, 'y_origin': last_found_smooth_data_origin, 'spline': last_found_spline}

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
                yield {'start': start, 'smooth_data': last_found_smooth_data, 'y_origin': last_found_smooth_data_origin, 'spline': last_found_spline}
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
                last_found_smooth_data = [dns(x) for x in x]
                last_found_smooth_data_origin = [block[x] for x in x]
                last_found_spline = dns

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
                y_origin = [block[x] for x in x]
                yield {'start': start, 'smooth_data': y, 'y_origin': y_origin, 'spline': dns}

                start = points_counter - len(block) + second_shot
                # сохранить данные от second_shot до конца окна
                x = [x for x in range(second_shot, len(block))]
                last_found_smooth_data = [dns(x) for x in x]
                last_found_smooth_data_origin = [block[x] for x in x]
                last_found_spline = dns

                # Сдвигаем окно
                block = block[second_shot:]
